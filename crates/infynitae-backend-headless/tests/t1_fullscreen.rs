//! Production path tests for protocol fullscreen (M9): as a real client sees it and as the
//! output shows it.
//!
//! Each test drives a Wayland client over the protocol and checks three things together: the
//! configures the client received (`probe.toplevel_configures`), the enter/leave it saw
//! (`probe.log`), and the pixels of the output, composited by the production render path with
//! the software renderer. Core's state alone is not the oracle.
//!
//! Output 640x480 at scale 1, camera at the Canvas origin. W is red, the other window blue.
//! A 200x100 window is centred by F-PLACE-1 at physical [220,420)x[190,290); a 120x80 one at
//! [260,380)x[200,280).

mod common;

use std::os::fd::AsFd;
use std::time::Duration;

use common::{Lockstep, output, output_k};
use infynitae_backend_headless::set_focus;
use infynitae_core::stage::{ElementId, Lifecycle};
use infynitae_wayland::convert;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, ExportMem, Offscreen};
use smithay::reexports::pixman::Image;
use smithay::utils::Rectangle;
use wayland_client::protocol::wl_shm;

const CLEAR: u32 = 0x0000_0000;
const RED: u32 = 0xFFFF_0000;
const BLUE: u32 = 0xFF00_00FF;
const FULLSCREEN: u32 = 2;
const ACTIVATED: u32 = 4;
/// `tiled_left`, `tiled_right`, `tiled_top`, `tiled_bottom`: a window that is not fullscreen is
/// tiled on every side (SQUARE_CORNERS).
const TILED: [u32; 4] = [5, 6, 7, 8];

/// The states of a window that is not fullscreen.
fn windowed_states(activated: bool) -> Vec<u32> {
    let mut s = if activated { vec![ACTIVATED] } else { vec![] };
    s.extend(TILED);
    s
}
const SIZE: (i32, i32) = (640, 480);

type Layer = ((i32, i32, i32, i32), u32);

struct T {
    ls: Lockstep,
    renderer: PixmanRenderer,
    image: Image<'static, 'static>,
}

impl T {
    fn new() -> T {
        T::with(Lockstep::new(Some(output((640, 480), 120))))
    }

    fn with(ls: Lockstep) -> T {
        let mut renderer = PixmanRenderer::new().expect("software renderer");
        let image = renderer
            .create_buffer(Fourcc::Argb8888, SIZE.into())
            .expect("render target");
        T {
            ls,
            renderer,
            image,
        }
    }

    fn adv(&mut self) {
        self.ls.pump();
        let t = self.ls.now + Duration::from_micros(16_667);
        self.ls.run_until(t);
    }

    /// Attaches a solid buffer of the surface's current size and commits.
    fn paint(&mut self, idx: usize, argb: u32) {
        let (w, h) = self.ls.probe.surfaces[idx].size;
        let len = (w * h * 4) as usize;
        let fd = smithay::reexports::rustix::fs::memfd_create(
            "infynitae-t1-fullscreen",
            smithay::reexports::rustix::fs::MemfdFlags::CLOEXEC,
        )
        .expect("memfd");
        let file = std::fs::File::from(fd);
        std::os::unix::fs::FileExt::write_all_at(&file, &argb.to_le_bytes().repeat(len / 4), 0)
            .expect("fill");
        let shm = self.ls.probe.shm.clone().expect("wl_shm");
        let pool = shm.create_pool(file.as_fd(), len as i32, &self.ls.qh, ());
        let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &self.ls.qh, ());
        let wl = &self.ls.probe.surfaces[idx].wl;
        wl.attach(Some(&buffer), 0, 0);
        wl.damage_buffer(0, 0, w, h);
        wl.commit();
        self.adv();
    }

    /// A mapped toplevel of `size`, painted `argb`.
    fn map(&mut self, label: &'static str, size: (i32, i32), argb: u32) -> (usize, ElementId) {
        let qh = self.ls.qh.clone();
        let idx = self.ls.probe.toplevel(&qh, label, size, 0);
        self.adv();
        self.paint(idx, argb);
        let id = self.ls.state.elements().last().expect("element");
        assert_eq!(
            self.ls.state.core().stage().state(id),
            Some(Lifecycle::Mapped)
        );
        (idx, id)
    }

    /// The client redraws at `size` (what it does after a configure it obeys).
    fn resize(&mut self, idx: usize, size: (i32, i32), argb: u32) {
        self.ls.probe.surfaces[idx].size = size;
        self.paint(idx, argb);
    }

    fn focus(&mut self, id: ElementId) {
        set_focus(&mut self.ls.state, &mut self.ls.input, Some(id)).expect("focus");
        self.adv();
    }

    fn configures(&self, idx: usize) -> Vec<((i32, i32), Vec<u32>)> {
        self.ls
            .probe
            .toplevel_configures
            .iter()
            .filter(|(i, _, _)| *i == idx)
            .map(|(_, size, states)| {
                // the states array is a set: its order on the wire carries no meaning
                let mut states = states.clone();
                states.sort_unstable();
                (*size, states)
            })
            .collect()
    }

    /// Sends a request and returns the configures it caused.
    fn ask(
        &mut self,
        idx: usize,
        request: impl FnOnce(&wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel),
    ) -> Vec<((i32, i32), Vec<u32>)> {
        let before = self.configures(idx).len();
        request(
            self.ls.probe.surfaces[idx]
                .toplevel
                .as_ref()
                .expect("toplevel"),
        );
        self.adv();
        self.configures(idx)[before..].to_vec()
    }

    /// One frame of an output through the production render path; (damage present, pixels).
    fn frame_of(&mut self, o: infynitae_core::output::OutputId, age: usize) -> (bool, Vec<u32>) {
        let mut target = self.renderer.bind(&mut self.image).expect("bind");
        let damage =
            convert::render_output(&mut self.ls.state, &mut self.renderer, &mut target, o, age)
                .expect("render_output");
        let mapping = self
            .renderer
            .copy_framebuffer(&target, Rectangle::from_size(SIZE.into()), Fourcc::Argb8888)
            .expect("read back");
        let bytes = self.renderer.map_texture(&mapping).expect("map");
        let pixels = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        (damage.is_some(), pixels)
    }

    fn frame(&mut self) -> Vec<u32> {
        let o = self.ls.output();
        self.frame_of(o, 0).1
    }

    /// The wl_output global name of the first output, as the client's log prints it.
    fn output_name(&self) -> u32 {
        self.ls.probe.outputs[0].0
    }

    fn log_since(&self, from: usize) -> Vec<String> {
        self.ls.probe.log[from..]
            .iter()
            .filter(|l| l.starts_with("enter") || l.starts_with("leave"))
            .cloned()
            .collect()
    }
}

/// Every pixel against a painter's oracle: the front-most layer covering it, or clear.
fn assert_layers(pixels: &[u32], layers: &[Layer], what: &str) {
    assert_eq!(pixels.len(), (SIZE.0 * SIZE.1) as usize);
    for y in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            let want = layers
                .iter()
                .find(|((x0, y0, x1, y1), _)| x >= *x0 && x < *x1 && y >= *y0 && y < *y1)
                .map_or(CLEAR, |(_, c)| *c);
            let got = pixels[(y * SIZE.0 + x) as usize];
            assert_eq!(
                got, want,
                "{what}: pixel ({x},{y}) is {got:#010x}, expected {want:#010x}"
            );
        }
    }
}

const W_WINDOWED: Layer = ((220, 190, 420, 290), RED);
const OTHER: Layer = ((260, 200, 380, 280), BLUE);

/// M9 rows 1 and 3 end to end. The focused client asks; it gets one configure with `fullscreen`
/// and the output's size; the other window leaves the output at once. While the client has not
/// resized, its 200x100 surface is shown centred with nothing else; after it draws 640x480 the
/// whole output is its. It asks to leave: one configure offering 200x100; the other window is
/// back at once; after it redraws at 200x100 the output is as it was, pixel for pixel. Keyboard
/// focus stays with it throughout and its anchor never moves.
#[test]
fn a_focused_client_goes_fullscreen_and_back() {
    let mut t = T::new();
    let (w, id) = t.map("W", (200, 100), RED);
    t.map("O", (120, 80), BLUE);
    t.focus(id);
    let n = t.output_name();
    let anchor = t.ls.state.core().stage().anchor(id);
    let windowed = t.frame();
    assert_layers(&windowed, &[OTHER, W_WINDOWED], "windowed");

    let log = t.ls.probe.log.len();
    let got = t.ask(w, |tl| tl.set_fullscreen(None));
    assert_eq!(
        got,
        vec![((640, 480), vec![FULLSCREEN, ACTIVATED])],
        "the grant"
    );
    assert_eq!(
        t.log_since(log),
        vec![format!("leave O {n}")],
        "the other window leaves"
    );
    // not resized yet: centred, alone (F-PRESENT-1); border fill is the clear colour
    assert_layers(&t.frame(), &[W_WINDOWED], "granted, not yet resized");

    t.resize(w, (640, 480), RED);
    assert_layers(&t.frame(), &[((0, 0, 640, 480), RED)], "fullscreen");
    assert_eq!(
        t.ls.state.core().stage().anchor(id),
        anchor,
        "the anchor moved"
    );
    assert_eq!(t.ls.probe.kb.active.as_deref(), Some("W"));
    assert_eq!(
        t.log_since(log),
        vec![format!("leave O {n}")],
        "nothing else entered or left"
    );

    // an unchanged fullscreen scene is not redrawn
    let o = t.ls.output();
    t.frame_of(o, 1);
    for _ in 0..3 {
        assert!(!t.frame_of(o, 1).0, "damage with nothing changed");
    }

    let log = t.ls.probe.log.len();
    let got = t.ask(w, |tl| tl.unset_fullscreen());
    assert_eq!(
        got,
        vec![((200, 100), windowed_states(true))],
        "the saved size is offered"
    );
    assert_eq!(
        t.log_since(log),
        vec![format!("enter O {n}")],
        "the other window returns"
    );
    t.resize(w, (200, 100), RED);
    assert_eq!(t.frame(), windowed, "exactly as before fullscreen");
    assert_eq!(t.ls.state.core().stage().anchor(id), anchor);
    assert_eq!(t.ls.probe.kb.active.as_deref(), Some("W"));
    assert!(t.ls.state.core().fullscreen().is_empty());
    // later configures carry no size: the offer was made once
    let got = t.ask(w, |tl| tl.set_maximized());
    assert_eq!(got, vec![((0, 0), windowed_states(true))]);
}

/// M9 row 2: a client that does not hold the focus is declined with one configure, and nothing
/// on the output changes.
#[test]
fn an_unfocused_client_is_declined_and_nothing_changes() {
    let mut t = T::new();
    let (w, _) = t.map("W", (200, 100), RED);
    let (_, other) = t.map("O", (120, 80), BLUE);
    t.focus(other);
    let before = t.frame();
    let log = t.ls.probe.log.len();
    assert_eq!(
        t.ask(w, |tl| tl.set_fullscreen(None)),
        vec![((0, 0), windowed_states(false))]
    );
    assert_eq!(t.log_since(log), Vec::<String>::new());
    assert_eq!(t.frame(), before);
    assert!(t.ls.state.core().fullscreen().is_empty());
}

/// Pre-map fullscreen (how xwayland-satellite presents a screen-sized X11 window, EC-13): the
/// request precedes the initial commit and gets no configure of its own; the initial configure
/// is the only one and carries `fullscreen` and the output's size. Until the client's first
/// buffer the output still shows the other window. The client maps at 640x480 and owns the
/// output. Leaving offers no size, because none was saved.
#[test]
fn a_client_that_asks_before_mapping_starts_fullscreen() {
    let mut t = T::new();
    t.map("O", (120, 80), BLUE);
    let n = t.output_name();
    let qh = t.ls.qh.clone();
    let w = t.ls.probe.toplevel_uncommitted(&qh, "W", (640, 480));
    t.ls.probe.surfaces[w]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    t.adv();
    assert_eq!(
        t.configures(w),
        vec![],
        "answered before the initial commit"
    );
    assert_layers(&t.frame(), &[OTHER], "before the initial commit");
    let log = t.ls.probe.log.len();
    // the initial commit; the probe draws its 640x480 buffer when the configure arrives
    t.ls.probe.surfaces[w].wl.commit();
    t.adv();
    assert_eq!(
        t.configures(w),
        vec![((640, 480), vec![FULLSCREEN])],
        "the initial configure"
    );
    t.paint(w, RED);
    assert_layers(&t.frame(), &[((0, 0, 640, 480), RED)], "mapped fullscreen");
    let entered: Vec<String> = t.log_since(log);
    assert!(entered.contains(&format!("leave O {n}")), "{entered:?}");
    assert!(entered.contains(&format!("enter W {n}")), "{entered:?}");
    let got = t.ask(w, |tl| tl.unset_fullscreen());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, (0, 0), "no size was saved, none is offered");
    assert!(!got[0].1.contains(&FULLSCREEN));
    // It has never been a window, so its first windowed size is where default placement
    // happens: a 200x100 window is centred, not left in the corner it filled the output from.
    t.resize(w, (200, 100), RED);
    assert_layers(&t.frame(), &[W_WINDOWED, OTHER], "its first windowed size");
    // and it stays there
    t.resize(w, (240, 100), RED);
    assert_layers(
        &t.frame(),
        &[((220, 190, 460, 290), RED), OTHER],
        "a later size keeps the anchor",
    );
}

/// A camera drag on a fullscreen output is allowed and moves the hidden camera only: the picture
/// does not change, the client receives no pointer event during the drag, and a pointer lock
/// the client holds stays active through the grant, the drag and the release.
#[test]
fn a_hidden_camera_drag_respects_the_presentation_and_a_pointer_lock() {
    use infynitae_backend_headless::{keyboard_key, pointer_button, pointer_motion};
    use infynitae_core::input::bindings::Modifiers;
    use infynitae_core::input::keys::KEY_LEFTMETA;
    let mut t = T::new();
    let (w, id) = t.map("W", (200, 100), RED);
    t.map("O", (120, 80), BLUE);
    t.focus(id);
    // The pointer starts at the view centre (320, 240), where the other window lies on top of W.
    // (−95, −45) takes it to (225, 195): inside W [220,420)x[190,290), outside O
    // [260,380)x[200,280). The motion gives W the pointer focus.
    pointer_motion(
        &mut t.ls.state,
        &mut t.ls.input,
        (-95.0, -45.0),
        (-95.0, -45.0),
        1000,
    )
    .unwrap();
    t.adv();
    assert_eq!(
        t.ls.probe
            .ptr
            .iter()
            .rev()
            .find(|l| l.starts_with("enter"))
            .map(String::as_str),
        Some("enter W 5,5"),
        "the pointer is not on W"
    );
    let qh = t.ls.qh.clone();
    t.ls.probe.lock_pointer(&qh, w, false);
    t.adv();
    let locked = |t: &T| t.ls.state.core().constraint().is_some();
    assert!(locked(&t), "the lock did not activate");

    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    assert!(locked(&t), "the grant ended the lock");
    let picture = t.frame();
    assert_layers(&picture, &[((0, 0, 640, 480), RED)], "fullscreen");
    let camera = t.ls.state.core().cameras().center(0);

    let events = t.ls.probe.ptr.len();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, KEY_LEFTMETA, true, 2000).unwrap();
    pointer_button(
        &mut t.ls.state,
        &mut t.ls.input,
        0x112,
        true,
        Modifiers::default(),
        3000,
    )
    .unwrap();
    pointer_motion(
        &mut t.ls.state,
        &mut t.ls.input,
        (-90.0, 40.0),
        (-90.0, 40.0),
        4000,
    )
    .unwrap();
    t.adv();
    let during: Vec<String> = t.ls.probe.ptr[events..]
        .iter()
        .filter(|l| *l != "frame")
        .cloned()
        .collect();
    assert_eq!(
        during,
        Vec::<String>::new(),
        "pointer events during the drag"
    );
    pointer_button(
        &mut t.ls.state,
        &mut t.ls.input,
        0x112,
        false,
        Modifiers::default(),
        5000,
    )
    .unwrap();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, KEY_LEFTMETA, false, 6000).unwrap();
    t.adv();
    assert_ne!(
        t.ls.state.core().cameras().center(0),
        camera,
        "the camera did not move"
    );
    assert_eq!(t.frame(), picture, "the drag changed what the output shows");
    assert!(locked(&t), "the drag ended the lock");

    t.ask(w, |tl| tl.unset_fullscreen());
    t.resize(w, (200, 100), RED);
    assert!(t.ls.state.core().fullscreen().is_empty());
    // the output now shows the camera where the drag left it: 90 px right, 40 px up of before
    assert_layers(
        &t.frame(),
        &[
            ((260 - 90, 200 + 40, 380 - 90, 280 + 40), BLUE),
            ((220 - 90, 190 + 40, 420 - 90, 290 + 40), RED),
        ],
        "the dragged camera's view",
    );
}

/// A client may ignore the size it is given. Its smaller surface is presented centred on the
/// output with nothing else drawn (xdg-shell: "position the surface in the center of the output
/// and compensate with border fill").
#[test]
fn a_client_that_keeps_its_size_is_centred_alone() {
    let mut t = T::new();
    let (w, id) = t.map("W", (200, 100), RED);
    t.map("O", (120, 80), BLUE);
    t.focus(id);
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (300, 200), RED); // not the 640x480 it was asked for
    // centred: ((640−300)/2, (480−200)/2) = (170, 140)
    assert_layers(
        &t.frame(),
        &[((170, 140, 470, 340), RED)],
        "smaller than the output",
    );
}

/// No stale state: a fullscreen client unmaps; the other window is back at once; the client
/// re-shows and is an ordinary window again, with an initial configure that is not fullscreen,
/// at the place it had before. Then it goes fullscreen again and is destroyed in the middle of
/// the transition, before it resized: the output returns to the other window and no state is
/// left.
#[test]
fn unmap_remap_and_destruction_leave_no_fullscreen_state() {
    let mut t = T::new();
    let (w, id) = t.map("W", (200, 100), RED);
    t.map("O", (120, 80), BLUE);
    t.focus(id);
    let n = t.output_name();
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    let log = t.ls.probe.log.len();
    t.ls.probe.unmap(w);
    t.adv();
    assert!(
        t.ls.state.core().fullscreen().is_empty(),
        "state survived the unmap"
    );
    assert!(t.log_since(log).contains(&format!("enter O {n}")));
    assert_layers(&t.frame(), &[OTHER], "after the unmap");
    // re-show: initial commit, then a 200x100 buffer
    let before = t.configures(w).len();
    t.ls.probe.surfaces[w].wl.commit();
    t.adv();
    let got = t.configures(w)[before..].to_vec();
    assert_eq!(
        got.len(),
        1,
        "one configure for the re-initial commit: {got:?}"
    );
    assert!(
        !got[0].1.contains(&FULLSCREEN),
        "the remap is fullscreen: {got:?}"
    );
    assert_eq!(got[0].0, (0, 0));
    t.resize(w, (200, 100), RED);
    assert_layers(
        &t.frame(),
        &[W_WINDOWED, OTHER],
        "remapped as a window, on top",
    );

    // destroyed mid-transition
    t.focus(id);
    let got = t.ask(w, |tl| tl.set_fullscreen(None));
    assert_eq!(got.len(), 1);
    assert!(got[0].1.contains(&FULLSCREEN));
    t.ls.probe.destroy_toplevel(w);
    t.adv();
    assert!(
        t.ls.state.core().fullscreen().is_empty(),
        "state survived the destruction"
    );
    assert_layers(&t.frame(), &[OTHER], "after the destruction");
    assert_eq!(t.ls.state.core().stage().z_order().len(), 1);
}

/// Output association: with two outputs the client names the second. It is presented there,
/// sized to that output, and the first output keeps showing it through its camera at its real
/// size. When the named output is unplugged the client is told to leave fullscreen with its
/// saved size.
#[test]
fn the_client_can_name_the_output() {
    let ls = Lockstep::with(
        infynitae_core::navigation::PersistedCameras::default(),
        &[
            output_k("A", (640, 480), 120, 60_000),
            output_k("B", (800, 600), 120, 60_000),
        ],
    );
    let mut t = T::with(ls);
    let (w, id) = t.map("W", (200, 100), RED);
    t.focus(id);
    let b = t.ls.state.core().outputs()[1].id;
    let wl_b = t.ls.probe.outputs[1].1.clone();
    let got = t.ask(w, |tl| tl.set_fullscreen(Some(&wl_b)));
    assert_eq!(
        got,
        vec![((800, 600), vec![FULLSCREEN, ACTIVATED])],
        "B's size"
    );
    assert_eq!(t.ls.state.core().presentation(b), Some(id));
    t.resize(w, (800, 600), RED);
    // A's camera still shows W at its anchor (−100,−50), now 800x600: from (220,190) to the edge
    assert_layers(
        &t.frame(),
        &[((220, 190, 640, 480), RED)],
        "output A, through its camera",
    );
    let before = t.configures(w).len();
    t.ls.disconnect(b);
    t.adv();
    let got = t.configures(w)[before..].to_vec();
    assert_eq!(
        got,
        vec![((200, 100), windowed_states(true))],
        "told to leave, with the saved size"
    );
    assert!(t.ls.state.core().fullscreen().is_empty());
}

const GREEN: u32 = 0xFF00_FF00;

impl T {
    /// Moves the pointer by `d` (output-logical px).
    fn move_by(&mut self, d: (f64, f64)) {
        static T_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(50_000);
        let t = T_US.fetch_add(1000, std::sync::atomic::Ordering::Relaxed);
        infynitae_backend_headless::pointer_motion(&mut self.ls.state, &mut self.ls.input, d, d, t)
            .expect("motion");
        self.adv();
    }

    /// A left click where the pointer is; returns the press serial the client received.
    fn click(&mut self) -> u32 {
        static T_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(900_000);
        for pressed in [true, false] {
            let t = T_US.fetch_add(1000, std::sync::atomic::Ordering::Relaxed);
            infynitae_backend_headless::pointer_button(
                &mut self.ls.state,
                &mut self.ls.input,
                0x110,
                pressed,
                infynitae_core::input::bindings::Modifiers::default(),
                t,
            )
            .expect("button");
            self.adv();
        }
        self.ls.probe.last_serial
    }

    fn pixel(&mut self, x: i32, y: i32) -> u32 {
        self.frame()[(y * SIZE.0 + x) as usize]
    }
}

/// A grabbing popup opened before the window goes fullscreen survives the grant with its grab,
/// and is presented with the window: at its place in the window's geometry, which the output's
/// origin now is. The pointer reaches it through the presentation view. A further popup near the
/// output's edge is constrained to the fullscreen output (C5.2). Leaving fullscreen keeps the
/// popup and its grab too.
#[test]
fn a_popup_survives_the_grant_and_is_presented_with_its_fullscreen_window() {
    let mut t = T::new();
    let (w, id) = t.map("W", (200, 100), RED);
    t.map("O", (120, 80), BLUE);
    t.focus(id);
    t.move_by((-95.0, -45.0)); // (225, 195): W-local (5, 5), outside the other window
    let serial = t.click();
    let qh = t.ls.qh.clone();
    let spec = |content, at: (i32, i32), slide| common::PopupSpec {
        content,
        shadow: 0,
        anchor_rect: (at.0, at.1, 1, 1),
        slide,
    };
    let p =
        t.ls.probe
            .popup_with_grab_serial(&qh, "P", w, spec((60, 40), (20, 20), false), serial);
    t.adv();
    t.paint(p, GREEN);
    assert!(t.ls.state.keyboard_is_grabbed(), "the popup holds no grab");
    // windowed: the popup is at W's origin (220, 190) + (20, 20)
    assert_eq!(t.pixel(245, 215), GREEN);

    let got = t.ask(w, |tl| tl.set_fullscreen(None));
    assert_eq!(got, vec![((640, 480), vec![FULLSCREEN, ACTIVATED])]);
    t.resize(w, (640, 480), RED);
    assert!(
        t.ls.probe.popup_dones.is_empty(),
        "the grant dismissed the popup"
    );
    assert!(
        t.ls.state.keyboard_is_grabbed(),
        "the grant ended the popup's grab"
    );
    assert_layers(
        &t.frame(),
        &[((20, 20, 80, 60), GREEN), ((0, 0, 640, 480), RED)],
        "the popup, presented with its fullscreen window",
    );
    // the pointer is at (225, 195); (−195, −165) puts it at (30, 30), popup-local (10, 10)
    let events = t.ls.probe.ptr.len();
    t.move_by((-195.0, -165.0));
    assert!(
        t.ls.probe.ptr[events..]
            .iter()
            .any(|l| l == "enter P 10,10"),
        "the pointer did not reach the popup: {:?}",
        &t.ls.probe.ptr[events..]
    );

    // a popup that would leave the output is slid back inside it: 600 + 100 > 640, 450 + 60 > 480
    let q =
        t.ls.probe
            .popup(&qh, "Q", w, spec((100, 60), (600, 450), true));
    t.adv();
    assert_eq!(
        t.ls.probe.surfaces[q].popup_configure,
        Some((540, 420, 100, 60)),
        "constrained to the fullscreen output"
    );

    let got = t.ask(w, |tl| tl.unset_fullscreen());
    assert_eq!(got, vec![((200, 100), windowed_states(true))]);
    t.resize(w, (200, 100), RED);
    assert!(
        t.ls.probe.popup_dones.is_empty(),
        "leaving fullscreen dismissed the popup"
    );
    assert!(t.ls.state.keyboard_is_grabbed());
    assert_eq!(
        t.pixel(245, 215),
        GREEN,
        "the popup is back at its place in the window"
    );
}

/// A grabbing popup opened on a fullscreen window obeys the compositor's dismissal rule: a
/// camera drag acquired on that output dismisses it first (INV-INPUT-2), topmost first, and no
/// grab is left. The client destroys what was dismissed and the output shows the window alone.
#[test]
fn a_popup_chain_on_a_fullscreen_window_is_dismissed_by_a_camera_drag() {
    use infynitae_backend_headless::{keyboard_key, pointer_button};
    use infynitae_core::input::bindings::Modifiers;
    use infynitae_core::input::keys::KEY_LEFTMETA;
    let mut t = T::new();
    t.ls.probe.auto_destroy_popups = true;
    let (w, id) = t.map("W", (200, 100), RED);
    t.focus(id);
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    t.move_by((1.0, 1.0));
    let serial = t.click();
    let qh = t.ls.qh.clone();
    let spec = |at: (i32, i32)| common::PopupSpec {
        content: (60, 40),
        shadow: 0,
        anchor_rect: (at.0, at.1, 1, 1),
        slide: false,
    };
    let p1 =
        t.ls.probe
            .popup_with_grab_serial(&qh, "P1", w, spec((100, 100)), serial);
    t.adv();
    t.paint(p1, GREEN);
    // a second level, grabbed with a later event inside the first: a key press
    keyboard_key(&mut t.ls.state, &mut t.ls.input, 30, true, 2_000_000).unwrap();
    t.adv();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, 30, false, 2_001_000).unwrap();
    t.adv();
    let s2 = t.ls.probe.last_serial;
    let p2 =
        t.ls.probe
            .popup_with_grab_serial(&qh, "P2", p1, spec((20, 20)), s2);
    t.adv();
    t.paint(p2, BLUE);
    assert!(t.ls.state.keyboard_is_grabbed() && t.ls.state.is_pointer_grabbed());
    assert_layers(
        &t.frame(),
        &[
            ((120, 120, 180, 160), BLUE),
            ((100, 100, 160, 140), GREEN),
            ((0, 0, 640, 480), RED),
        ],
        "a chain two deep over the fullscreen window",
    );

    keyboard_key(
        &mut t.ls.state,
        &mut t.ls.input,
        KEY_LEFTMETA,
        true,
        3_000_000,
    )
    .unwrap();
    pointer_button(
        &mut t.ls.state,
        &mut t.ls.input,
        0x112,
        true,
        Modifiers::default(),
        3_001_000,
    )
    .unwrap();
    t.adv();
    assert_eq!(t.ls.probe.popup_dones, vec!["P2", "P1"], "topmost first");
    pointer_button(
        &mut t.ls.state,
        &mut t.ls.input,
        0x112,
        false,
        Modifiers::default(),
        3_002_000,
    )
    .unwrap();
    keyboard_key(
        &mut t.ls.state,
        &mut t.ls.input,
        KEY_LEFTMETA,
        false,
        3_003_000,
    )
    .unwrap();
    t.adv();
    assert!(!t.ls.state.keyboard_is_grabbed() && !t.ls.state.is_pointer_grabbed());
    assert_layers(&t.frame(), &[((0, 0, 640, 480), RED)], "the window alone");
    assert_eq!(t.ls.state.core().presentation(t.ls.output()), Some(id));
    assert_eq!(
        t.ls.probe.kb.active.as_deref(),
        Some("W"),
        "keyboard back on the window"
    );
}

/// A fullscreen window's commits under a still pointer send it no pointer motion (owner,
/// 2026-10-25: the cursor never hid over a fullscreen video). Core re-sends the true position
/// after each commit of a presented window, whose geometry may have changed; when focus, origin
/// and position are unchanged the client is told nothing, or a player that hides the cursor
/// after a still moment takes the commits for movement. A real motion is still delivered.
#[test]
fn a_fullscreen_windows_frames_under_a_still_pointer_send_no_motion() {
    use infynitae_backend_headless::pointer_motion;
    let mut t = T::new();
    let (w, id) = t.map("W", (200, 100), RED);
    t.focus(id);
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    pointer_motion(
        &mut t.ls.state,
        &mut t.ls.input,
        (5.0, 5.0),
        (5.0, 5.0),
        1000,
    )
    .unwrap();
    t.adv();
    assert!(
        t.ls.probe.ptr.iter().any(|l| l.starts_with("enter W")),
        "the pointer is not on W: {:?}",
        t.ls.probe.ptr
    );
    let before = t.ls.probe.ptr.len();
    for _ in 0..10 {
        t.paint(w, RED);
    }
    assert_eq!(
        t.ls.probe.ptr[before..].to_vec(),
        Vec::<String>::new(),
        "pointer events from frames alone"
    );
    pointer_motion(
        &mut t.ls.state,
        &mut t.ls.input,
        (3.0, 0.0),
        (3.0, 0.0),
        2000,
    )
    .unwrap();
    t.adv();
    assert!(
        t.ls.probe.ptr[before..]
            .iter()
            .any(|l| l.starts_with("motion")),
        "a real motion was not delivered: {:?}",
        &t.ls.probe.ptr[before..]
    );
}

/// Suppressing a presentation reconciles visibility and membership (INV-RENDER-3).
/// Fullscreen W on O, a window X in O's camera view; Super+Tab to X makes X entered on O;
/// the reverse (unsuppress) leaves X.
#[test]
fn focus_cycle_suppress_and_unsuppress_recomputes_visibility() {
    use infynitae_backend_headless::keyboard_key;
    use infynitae_core::input::keys::{KEY_LEFTMETA, KEY_TAB};

    let mut t = T::new();
    t.ls.state.core_mut().set_fullscreen_pan(true);
    let (w, id_w) = t.map("W", (200, 100), RED);
    let (_x, _id_x) = t.map("X", (120, 80), BLUE);
    t.focus(id_w);
    let n = t.output_name();

    // 1. Fullscreen W on output O
    let log = t.ls.probe.log.len();
    let got = t.ask(w, |tl| tl.set_fullscreen(None));
    assert_eq!(got, vec![((640, 480), vec![FULLSCREEN, ACTIVATED])]);
    assert_eq!(
        t.log_since(log),
        vec![format!("leave X {n}")],
        "X leaves output O when W enters fullscreen presentation"
    );
    assert_eq!(t.ls.state.core().presentation(t.ls.output()), Some(id_w));
    assert!(!t.ls.state.core().is_presentation_suppressed(t.ls.output()));

    // 2. Super+Tab cycles focus to X.
    // This suppresses fullscreen presentation on O: view switches to camera view (where X is located).
    // X must receive wl_surface.enter on output O!
    let log = t.ls.probe.log.len();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, KEY_LEFTMETA, true, 1000).unwrap();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, KEY_TAB, true, 2000).unwrap();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, KEY_TAB, false, 2500).unwrap();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, KEY_LEFTMETA, false, 3000).unwrap();
    t.adv();

    assert!(t.ls.state.core().is_presentation_suppressed(t.ls.output()));
    let entered = t.log_since(log);
    assert!(
        entered.contains(&format!("enter X {n}")),
        "Super+Tab to X makes X entered on O; got: {entered:?}"
    );

    // 3. Super+Tab again cycles focus back to W.
    // This unsuppresses fullscreen presentation on O: exclusive presentation returns.
    // X must receive wl_surface.leave on output O!
    let log = t.ls.probe.log.len();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, KEY_LEFTMETA, true, 4000).unwrap();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, KEY_TAB, true, 5000).unwrap();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, KEY_TAB, false, 5500).unwrap();
    keyboard_key(&mut t.ls.state, &mut t.ls.input, KEY_LEFTMETA, false, 6000).unwrap();
    t.adv();

    assert!(!t.ls.state.core().is_presentation_suppressed(t.ls.output()));
    let left = t.log_since(log);
    assert!(
        left.contains(&format!("leave X {n}")),
        "the reverse (unsuppress) leaves X; got: {left:?}"
    );
}
