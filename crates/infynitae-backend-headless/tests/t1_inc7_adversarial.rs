//! INC-7 (experimental) adversarial coverage of protocol fullscreen and of popups under it, on the
//! production path: a real client over the protocol, the output's pixels composited by the
//! production render path with the software renderer.
//!
//! Every test names the invariant it exercises. The clients here misbehave in ways the protocol
//! allows: they answer late or never, go away in the middle of a transition, toggle faster than
//! they draw, and draw sizes they were not asked for.
//!
//! Output 640x480 at scale 1, camera at the Canvas origin, pointer at the view centre (320,240).
//! W is red and 200x100, centred by F-PLACE-1 at physical [220,420)x[190,290); O is blue and
//! 120x80 at [260,380)x[200,280), mapped after W and so above it.

mod common;

use std::os::fd::AsFd;
use std::time::Duration;

use common::input::{BTN_LEFT, BTN_MIDDLE, none};
use common::{Lockstep, PopupSpec, output, output_k};
use infynitae_backend_headless::{keyboard_key, pointer_button, pointer_motion, set_focus};
use infynitae_core::input::keys::{KEY_A, KEY_LEFTMETA, KEY_N};
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::OutputId;
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
const GREEN: u32 = 0xFF00_FF00;
const BLUE: u32 = 0xFF00_00FF;
const YELLOW: u32 = 0xFFFF_FF00;
const FULLSCREEN: u32 = 2;
const ACTIVATED: u32 = 4;
const SIZE: (i32, i32) = (640, 480);

type Layer = ((i32, i32, i32, i32), u32);
type Configure = ((i32, i32), Vec<u32>);

const W_WINDOWED: Layer = ((220, 190, 420, 290), RED);
const OTHER: Layer = ((260, 200, 380, 280), BLUE);
const W_FULL: Layer = ((0, 0, 640, 480), RED);

struct T {
    ls: Lockstep,
    renderer: PixmanRenderer,
    image: Image<'static, 'static>,
    t: u64,
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
            t: 10_000,
        }
    }

    /// W (focused) and O, as the module documentation places them.
    fn scene() -> (T, usize, ElementId, ElementId) {
        let mut t = T::new();
        let (w, id) = t.map("W", (200, 100), RED);
        let (_, other) = t.map("O", (120, 80), BLUE);
        t.focus(id);
        (t, w, id, other)
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
            "infynitae-t1-inc7-adversarial",
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

    fn motion(&mut self, d: (f64, f64)) {
        self.t += 1000;
        pointer_motion(&mut self.ls.state, &mut self.ls.input, d, d, self.t).expect("motion");
        self.adv();
    }

    fn button(&mut self, b: u32, pressed: bool) {
        self.t += 1000;
        pointer_button(
            &mut self.ls.state,
            &mut self.ls.input,
            b,
            pressed,
            none(),
            self.t,
        )
        .expect("button");
        self.adv();
    }

    fn key(&mut self, k: u32, pressed: bool) {
        self.t += 1000;
        keyboard_key(&mut self.ls.state, &mut self.ls.input, k, pressed, self.t).expect("key");
        self.adv();
    }

    fn configures(&self, idx: usize) -> Vec<Configure> {
        self.ls
            .probe
            .toplevel_configures
            .iter()
            .filter(|(i, _, _)| *i == idx)
            .map(|(_, size, states)| {
                let mut states = states.clone();
                states.sort_unstable();
                (*size, states)
            })
            .collect()
    }

    fn toplevel(
        &self,
        idx: usize,
    ) -> &wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel {
        self.ls.probe.surfaces[idx]
            .toplevel
            .as_ref()
            .expect("toplevel")
    }

    /// Sends a request and returns the configures it caused.
    fn ask(
        &mut self,
        idx: usize,
        request: impl FnOnce(&wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel),
    ) -> Vec<Configure> {
        let before = self.configures(idx).len();
        request(self.toplevel(idx));
        self.adv();
        self.configures(idx)[before..].to_vec()
    }

    /// Acknowledges a configure held back by `hold_acks`.
    fn ack(&mut self, idx: usize, serial: u32) {
        self.ls.probe.surfaces[idx]
            .xdg
            .as_ref()
            .expect("xdg_surface")
            .ack_configure(serial);
    }

    fn frame_of(&mut self, o: OutputId, age: usize) -> (bool, Vec<u32>) {
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

    fn output_name(&self) -> u32 {
        self.ls.probe.outputs[0].0
    }

    /// The enter and leave events of the surface labelled `label` since `from`.
    fn membership_since(&self, from: usize, label: &str) -> Vec<String> {
        self.ls.probe.log[from..]
            .iter()
            .filter(|l| {
                l.starts_with(&format!("enter {label} "))
                    || l.starts_with(&format!("leave {label} "))
            })
            .cloned()
            .collect()
    }

    /// What must never change because a client asked for something: every anchor, the camera,
    /// the stacking order (INV-PRES-1, INV-CAMERA-4).
    fn authority(&self) -> String {
        let core = self.ls.state.core();
        let anchors: Vec<_> = core
            .stage()
            .z_order()
            .iter()
            .map(|e| (*e, core.stage().anchor(*e)))
            .collect();
        format!("{anchors:?} {:?}", core.cameras().center(0))
    }

    fn core_is_clean(&self) {
        let core = self.ls.state.core();
        assert!(core.fullscreen().is_empty(), "{:?}", core.fullscreen());
        for o in core.outputs() {
            assert_eq!(core.presentation(o.id), None);
        }
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

fn granted() -> Vec<Configure> {
    vec![((640, 480), vec![FULLSCREEN, ACTIVATED])]
}

// ---------------------------------------------------------------------------------------------
// Transitions faster than the client draws
// ---------------------------------------------------------------------------------------------

/// Invariant (M9 "configure with the saved size"; C5.2 "offering the saved size"): the size
/// offered when fullscreen ends is the client's last *windowed* size. A client that is asked to
/// leave and asks to enter again before it has redrawn windowed still holds a fullscreen-sized
/// buffer; that size is not a windowed size and must not replace the one saved.
#[test]
fn re_entering_before_redrawing_windowed_keeps_the_windowed_size() {
    let (mut t, w, _, _) = T::scene();
    assert_eq!(t.ask(w, |tl| tl.set_fullscreen(None)), granted());
    t.resize(w, (640, 480), RED);
    assert_eq!(
        t.ask(w, |tl| tl.unset_fullscreen()),
        vec![((200, 100), vec![ACTIVATED])]
    );
    // the user toggles again before the client has drawn its 200x100 buffer
    assert_eq!(t.ask(w, |tl| tl.set_fullscreen(None)), granted());
    assert_eq!(
        t.ask(w, |tl| tl.unset_fullscreen()),
        vec![((200, 100), vec![ACTIVATED])],
        "the fullscreen size was saved as the windowed size"
    );
    t.resize(w, (200, 100), RED);
    assert_layers(&t.frame(), &[OTHER, W_WINDOWED], "back as it was");
    t.core_is_clean();
}

/// Invariant (INV-PRES-1): every `set_fullscreen` / `unset_fullscreen` is answered with exactly
/// one configure, however fast they come and whether or not the client draws in between; the
/// other window leaves and returns exactly once per transition; nothing a client asks for moves
/// an anchor, the camera or the stacking order.
#[test]
fn fifty_toggles_without_a_single_redraw() {
    let (mut t, w, _, _) = T::scene();
    let n = t.output_name();
    let windowed = t.frame();
    let authority = t.authority();
    let log = t.ls.probe.log.len();
    for i in 0..50 {
        assert_eq!(t.ask(w, |tl| tl.set_fullscreen(None)), granted(), "set {i}");
        assert_eq!(
            t.ask(w, |tl| tl.unset_fullscreen()),
            vec![((200, 100), vec![ACTIVATED])],
            "unset {i}"
        );
    }
    let mut want = Vec::new();
    for _ in 0..50 {
        want.push(format!("leave O {n}"));
        want.push(format!("enter O {n}"));
    }
    assert_eq!(t.membership_since(log, "O"), want);
    assert_eq!(t.membership_since(log, "W"), Vec::<String>::new());
    assert_eq!(t.authority(), authority);
    assert_eq!(t.frame(), windowed);
    t.core_is_clean();
}

/// The same, with both requests of each pair in one protocol batch (no dispatch in between):
/// two requests, two configures, in order.
#[test]
fn a_set_and_an_unset_in_one_batch_get_one_configure_each() {
    let (mut t, w, _, _) = T::scene();
    let got = t.ask(w, |tl| {
        tl.set_fullscreen(None);
        tl.unset_fullscreen();
        tl.set_fullscreen(None);
    });
    assert_eq!(
        got,
        vec![
            ((640, 480), vec![FULLSCREEN, ACTIVATED]),
            ((200, 100), vec![ACTIVATED]),
            ((640, 480), vec![FULLSCREEN, ACTIVATED]),
        ]
    );
    assert_eq!(t.ls.state.core().fullscreen().len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Clients that answer late, wrongly or never
// ---------------------------------------------------------------------------------------------

/// Invariant (INV-STAGE-4 "purge every reference"; INV-PRES-1 exit at destruction): a client
/// destroyed after the grant was sent and before it acknowledged it leaves no fullscreen state,
/// and the output returns to the other window at once.
#[test]
fn destroyed_before_acknowledging_the_grant() {
    let (mut t, w, _, _) = T::scene();
    let n = t.output_name();
    t.ls.probe.hold_acks = true;
    assert_eq!(t.ask(w, |tl| tl.set_fullscreen(None)), granted());
    assert_eq!(
        t.ls.probe.held_acks.len(),
        1,
        "one configure to acknowledge"
    );
    let log = t.ls.probe.log.len();
    t.ls.probe.destroy_toplevel(w);
    t.adv();
    t.core_is_clean();
    assert_eq!(t.membership_since(log, "O"), vec![format!("enter O {n}")]);
    assert_layers(&t.frame(), &[OTHER], "after the destruction");
    assert_eq!(t.ls.state.core().stage().z_order().len(), 1);
}

/// Invariant (M1 "fullscreen dropped (M9)" on the null-buffer commit; C5.2 "on remap the client
/// must request again"): a client that unmaps without ever acknowledging the grant is an
/// ordinary unmapped toplevel; its re-show gets one initial configure that is not fullscreen.
#[test]
fn unmapped_before_acknowledging_the_grant() {
    let (mut t, w, id, _) = T::scene();
    let n = t.output_name();
    t.ls.probe.hold_acks = true;
    assert_eq!(t.ask(w, |tl| tl.set_fullscreen(None)), granted());
    let log = t.ls.probe.log.len();
    let before = t.configures(w).len();
    t.ls.probe.unmap(w);
    t.adv();
    t.core_is_clean();
    assert_eq!(
        t.ls.state.core().stage().state(id),
        Some(Lifecycle::Unmapped)
    );
    assert_eq!(t.membership_since(log, "O"), vec![format!("enter O {n}")]);
    assert_layers(&t.frame(), &[OTHER], "after the unmap");
    for (size, states) in &t.configures(w)[before..] {
        assert!(!states.contains(&FULLSCREEN), "{size:?} {states:?}");
    }
    // re-show
    t.ls.probe.hold_acks = false;
    let before = t.configures(w).len();
    t.ls.probe.surfaces[w].wl.commit();
    t.adv();
    assert_eq!(t.configures(w)[before..].to_vec(), vec![((0, 0), vec![])]);
    t.resize(w, (200, 100), RED);
    assert_layers(&t.frame(), &[W_WINDOWED, OTHER], "re-shown, on top");
    t.core_is_clean();
}

/// Invariant (INV-PRES-1: Core's decision is the state; the adapter keeps none): a client that
/// acknowledges the *grant* after the release was already sent, and draws fullscreen-sized,
/// is a large ordinary window. It does not get the output back.
#[test]
fn a_stale_acknowledgement_does_not_bring_fullscreen_back() {
    let (mut t, w, _, _) = T::scene();
    t.ls.probe.hold_acks = true;
    assert_eq!(t.ask(w, |tl| tl.set_fullscreen(None)), granted());
    assert_eq!(
        t.ask(w, |tl| tl.unset_fullscreen()),
        vec![((200, 100), vec![ACTIVATED])]
    );
    let held = t.ls.probe.held_acks.clone();
    assert_eq!(held.len(), 2);
    // the client answers the first configure only, as if the second had not arrived yet
    t.ack(w, held[0].1);
    t.resize(w, (640, 480), RED);
    t.core_is_clean();
    // W is 640x480 at its anchor (-100,-50): physical (220,190) to the output's edge, under O
    assert_layers(
        &t.frame(),
        &[OTHER, ((220, 190, 640, 480), RED)],
        "an oversized window, not a presentation",
    );
    // then it catches up
    t.ack(w, held[1].1);
    t.resize(w, (200, 100), RED);
    assert_layers(&t.frame(), &[OTHER, W_WINDOWED], "caught up");
}

/// P9b: a window whose first map is in fullscreen is placed at its first windowed presentation,
/// the first commit after it acknowledged the configure that ends fullscreen. Frames it commits
/// before acknowledging — at the fullscreen size, or already at its windowed size — leave its
/// provisional anchor (its origin at the camera centre, the view's centre here); after the
/// acknowledgement it is centred in the view. No configure is sent for the placement.
#[test]
fn placement_after_fullscreen_waits_for_the_acknowledgement() {
    let mut t = T::new();
    let qh = t.ls.qh.clone();
    let x = t.ls.probe.toplevel_uncommitted(&qh, "X", (640, 480));
    t.toplevel(x).set_fullscreen(None);
    t.ls.probe.surfaces[x].wl.commit();
    t.adv();
    t.adv();
    let id = t.ls.state.elements().last().expect("X");
    assert_eq!(t.ls.state.core().presentation(t.ls.output()), Some(id));
    t.focus(id);

    t.ls.probe.hold_acks = true;
    t.ask(x, |tl| tl.unset_fullscreen());
    let configures = t.configures(x).len();
    t.resize(x, (640, 480), GREEN);
    t.resize(x, (200, 100), GREEN);
    assert_eq!(t.ls.state.core().placements_pending(), vec![id]);
    assert_layers(
        &t.frame(),
        &[((320, 240, 520, 340), GREEN)],
        "not placed before the acknowledgement",
    );
    let held = t.ls.probe.held_acks.clone();
    t.ls.probe.hold_acks = false;
    t.ack(x, held.last().expect("held").1);
    t.resize(x, (200, 100), GREEN);
    assert!(t.ls.state.core().placements_pending().is_empty());
    assert_layers(
        &t.frame(),
        &[((220, 190, 420, 290), GREEN)],
        "placed at the first commit after the acknowledgement",
    );
    assert_eq!(
        t.configures(x).len(),
        configures,
        "the placement configured X"
    );
}

/// Invariant (INV-PRES-1; M2 "exactly one configure"): a client that never acknowledges and
/// never redraws is presented as it is (centred, alone), is not sent the configure again, and
/// can still leave: one configure, and the output is exactly as it was.
#[test]
fn a_client_that_never_answers_the_grant() {
    let (mut t, w, _, _) = T::scene();
    let windowed = t.frame();
    t.ls.probe.hold_acks = true;
    assert_eq!(t.ask(w, |tl| tl.set_fullscreen(None)), granted());
    let before = t.configures(w).len();
    for _ in 0..30 {
        t.adv();
    }
    assert_eq!(t.configures(w).len(), before, "the configure was repeated");
    assert_layers(&t.frame(), &[W_WINDOWED], "presented as it is");
    let o = t.ls.output();
    t.frame_of(o, 1);
    assert!(!t.frame_of(o, 1).0, "damage with nothing changed");
    assert_eq!(
        t.ask(w, |tl| tl.unset_fullscreen()),
        vec![((200, 100), vec![ACTIVATED])]
    );
    assert_eq!(t.frame(), windowed);
    t.core_is_clean();
}

/// Invariant (F-PRESENT-1: "a surface larger than the output is origin-aligned and cropped"): a
/// client that draws larger than it was asked to fills the output from its window-geometry
/// origin; the pointer at the output's corner is at the surface's corner.
#[test]
fn an_oversized_buffer_is_origin_aligned_and_cropped() {
    let (mut t, w, id, _) = T::scene();
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (800, 600), RED);
    assert_layers(&t.frame(), &[W_FULL], "cropped to the output");
    let o = t.ls.output();
    let hit =
        t.ls.state
            .core_mut()
            .hit_test(
                o,
                infynitae_core::geom::OutputLogicalPoint { x: 0.0, y: 0.0 },
            )
            .expect("a hit at the output's origin");
    assert_eq!(hit.element, id);
    assert_eq!((hit.local.x >> 16, hit.local.y >> 16), (0, 0));
    let hit =
        t.ls.state
            .core_mut()
            .hit_test(
                o,
                infynitae_core::geom::OutputLogicalPoint { x: 639.0, y: 479.0 },
            )
            .expect("a hit at the output's far corner");
    assert_eq!((hit.local.x >> 16, hit.local.y >> 16), (639, 479));
}

/// Invariant (M1; INV-STAGE-5 "anchor kept"; no state outlives its element's mapping): twenty
/// rounds of fullscreen, unmap and re-show leave no fullscreen state, the same anchor, the same
/// number of elements, and as many returns of the other window as departures.
#[test]
fn twenty_rounds_of_fullscreen_unmap_and_reshow() {
    let (mut t, w, id, _) = T::scene();
    let n = t.output_name();
    let anchor = t.ls.state.core().stage().anchor(id);
    let log = t.ls.probe.log.len();
    for i in 0..20 {
        t.focus(id);
        assert_eq!(
            t.ask(w, |tl| tl.set_fullscreen(None)),
            granted(),
            "round {i}"
        );
        t.resize(w, (640, 480), RED);
        assert_eq!(t.ls.state.core().presentation(t.ls.output()), Some(id));
        t.ls.probe.unmap(w);
        t.adv();
        t.core_is_clean();
        let before = t.configures(w).len();
        t.ls.probe.surfaces[w].wl.commit();
        t.adv();
        assert_eq!(
            t.configures(w)[before..].to_vec(),
            vec![((0, 0), vec![])],
            "round {i}: the re-show"
        );
        t.resize(w, (200, 100), RED);
        assert_eq!(t.ls.state.core().stage().anchor(id), anchor, "round {i}");
    }
    let o_log = t.membership_since(log, "O");
    let leaves = o_log.iter().filter(|l| l.starts_with("leave")).count();
    let enters = o_log.iter().filter(|l| l.starts_with("enter")).count();
    assert_eq!((leaves, enters), (20, 20), "{o_log:?}");
    assert_eq!(*o_log.last().unwrap(), format!("enter O {n}"));
    assert_eq!(t.ls.state.core().stage().z_order().len(), 2);
    assert_eq!(t.ls.state.elements().count(), 2);
    assert_layers(&t.frame(), &[W_WINDOWED, OTHER], "after twenty rounds");
}

// ---------------------------------------------------------------------------------------------
// Focus, pointer and camera
// ---------------------------------------------------------------------------------------------

/// Invariant (C5.2 "hit-testing and input on O go through the presentation view"; INV-HIT-1): a
/// window hidden by a presentation receives no pointer input. The pointer rests where O is
/// drawn; when W takes the output the pointer is on W, and a click there is W's.
#[test]
fn a_hidden_window_gets_no_pointer_input() {
    let (mut t, w, id, _) = T::scene();
    t.motion((1.0, 0.0));
    t.motion((-1.0, 0.0));
    let entered = |t: &T| {
        t.ls.probe
            .ptr
            .iter()
            .rev()
            .find(|l| l.starts_with("enter"))
            .cloned()
            .expect("an enter")
    };
    assert!(entered(&t).starts_with("enter O "), "{}", entered(&t));
    t.ask(w, |tl| tl.set_fullscreen(None));
    assert!(
        entered(&t).starts_with("enter W "),
        "one true position when the presentation starts: {}",
        entered(&t)
    );
    t.resize(w, (640, 480), RED);
    let mark = t.ls.probe.ptr.len();
    t.button(BTN_LEFT, true);
    t.button(BTN_LEFT, false);
    let since: Vec<&str> = t.ls.probe.ptr[mark..].iter().map(|s| s.as_str()).collect();
    assert!(
        since.iter().any(|l| l.contains("pressed")),
        "the click was not delivered: {since:?}"
    );
    assert!(!since.iter().any(|l| l.starts_with("enter O")), "{since:?}");
    assert_eq!(t.ls.state.core().focus().target(), Some(id));
    assert_eq!(t.ls.probe.kb.active.as_deref(), Some("W"));
}

/// xdg-shell: "A newly created xdg_popup will be stacked on top of all previously created
/// xdg_popup surfaces associated with the same xdg_toplevel" (P10 #3). Three popups of W, below
/// O so only they and W can show: A, then B overlapping it (B in front), then A1, a child of A
/// created after B and overlapping it (A1 in front of B, although its parent is behind B).
/// Smithay 0.7.0 orders A's subtree before B (W-SM-20); the adapter orders by creation.
#[test]
fn a_newer_popup_is_stacked_above_every_older_one() {
    let (mut t, w, _, _) = T::scene();
    let qh = t.ls.qh.clone();
    let spec = |content, at: (i32, i32)| PopupSpec {
        content,
        shadow: 0,
        anchor_rect: (at.0, at.1, 1, 1),
        slide: false,
    };
    let a = t.ls.probe.popup(&qh, "A", w, spec((60, 40), (10, 95)));
    t.adv();
    t.paint(a, GREEN);
    let b = t.ls.probe.popup(&qh, "B", w, spec((60, 40), (40, 105)));
    t.adv();
    t.paint(b, YELLOW);
    let a1 = t.ls.probe.popup(&qh, "A1", a, spec((40, 20), (20, 15)));
    t.adv();
    t.paint(a1, BLUE);
    // W at (220, 190): A (230,285)-(290,325), B (260,295)-(320,335), A1 (250,300)-(290,320)
    assert_layers(
        &t.frame(),
        &[
            ((250, 300, 290, 320), BLUE),
            ((260, 295, 320, 335), YELLOW),
            ((230, 285, 290, 325), GREEN),
            OTHER,
            W_WINDOWED,
        ],
        "newest popup in front",
    );
    // hit-testing follows the same order (one snapshot drives both)
    let o = t.ls.output();
    let at = |t: &mut T, x: f64, y: f64| {
        let p = infynitae_core::geom::OutputLogicalPoint { x, y };
        t.ls.state.core_mut().hit_test(o, p).map(|h| h.surface)
    };
    let (in_a1, in_b_over_a, in_a) = (
        at(&mut t, 270.5, 310.5),
        at(&mut t, 270.5, 297.5),
        at(&mut t, 240.5, 300.5),
    );
    assert!(in_a1.is_some() && in_b_over_a.is_some() && in_a.is_some());
    assert_ne!(in_a1, in_b_over_a, "A1 is not in front of B");
    assert_ne!(in_b_over_a, in_a, "B is not in front of A");
    assert_eq!(at(&mut t, 252.5, 310.5), in_a1);
}

/// Audit finding A-5. `wl_pointer` event times are milliseconds on one clock, so a client sees
/// them non-decreasing (toolkits measure double clicks and pointer velocity with them). A pointer
/// event Core synthesises into the stream (M10's one true position, after a presentation, output
/// or interaction change) carries the input clock's time: that of the latest physical event Core
/// accepted. The harness stamps its events 1 ms apart, so that time is known exactly. A
/// synthesised event that changes the pointer focus reaches the client as `leave`/`enter`, which
/// carry no time, so each test keeps the pointer over W alone, at (240, 240).
impl T {
    /// The scene with the pointer over W alone.
    fn scene_over_w() -> (T, usize, ElementId) {
        let (mut t, w, id, _) = T::scene();
        t.motion((-80.0, 0.0));
        (t, w, id)
    }

    /// The latest input time on the wire.
    fn input_ms(&self) -> u32 {
        (self.t / 1000) as u32
    }

    /// Asserts every `wl_pointer` time the client has received is at least the one before, and
    /// that every event since `from` carries `ms`. Returns how many arrived since `from`.
    fn pointer_times_since(&self, from: usize, ms: u32, what: &str) -> usize {
        let times = &self.ls.probe.ptr_times;
        for pair in times.windows(2) {
            assert!(
                pair[1].1 >= pair[0].1,
                "{what}: a pointer event went back in time: {times:?}"
            );
        }
        for (kind, time) in &times[from..] {
            assert_eq!(*time, ms, "{what}: {kind} at {time}, not {ms}: {times:?}");
        }
        times.len() - from
    }
}

#[test]
fn pointer_event_times_never_go_back_when_a_presentation_starts_or_ends() {
    let (mut t, w, _) = T::scene_over_w();
    t.motion((1.0, 0.0));
    let from = t.ls.probe.ptr_times.len();
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    let granted = t.pointer_times_since(from, t.input_ms(), "the grant");
    t.motion((1.0, 0.0));
    let from = t.ls.probe.ptr_times.len();
    t.ask(w, |tl| tl.unset_fullscreen());
    t.resize(w, (200, 100), RED);
    let ended = t.pointer_times_since(from, t.input_ms(), "the end");
    // the pointer was over W both times: each change reached it as one true position
    assert!(
        granted > 0 && ended > 0,
        "nothing synthesised: {granted}, {ended}"
    );
}

/// A-5 at output changes: the output reconfigured under the pointer, a second output connected,
/// the pointer's output removed (relocation), every output removed (NoOutput) and one connected
/// again (the pointer comes back). Every event a change synthesises carries the latest input time.
#[test]
fn pointer_event_times_never_go_back_at_output_changes() {
    let (mut t, _, _) = T::scene_over_w();
    let a = t.ls.output();
    let mut synthesised = 0;
    let mut step = |t: &mut T, what: &str, f: &mut dyn FnMut(&mut T)| {
        let from = t.ls.probe.ptr_times.len();
        f(t);
        t.adv();
        synthesised += t.pointer_times_since(from, t.input_ms(), what);
    };
    step(&mut t, "reconfigured", &mut |t| {
        t.ls.reconfigure(a, &output_k("A", (640, 480), 120, 144_000))
    });
    let b = t.ls.connect(&output_k("B", (640, 480), 120, 60_000));
    t.motion((-1.0, 0.0));
    step(&mut t, "relocated", &mut |t| t.ls.disconnect(a));
    t.motion((1.0, 0.0));
    step(&mut t, "no output", &mut |t| t.ls.disconnect(b));
    step(&mut t, "an output again", &mut |t| {
        t.ls.connect(&output_k("C", (640, 480), 120, 60_000));
    });
    // the reconfiguration happens under the pointer, over W: one timed motion. Relocation and
    // reconnection move the pointer off W here, so they reach the client as leave and enter.
    assert!(synthesised >= 1, "no change synthesised a pointer event");
}

/// A-5 at the end of a camera drag (the true position carries the release's time) and under a
/// popup's pointer grab (the synthesised event goes to the grab, at the latest input time).
#[test]
fn pointer_event_times_never_go_back_after_a_drag_and_under_a_grab() {
    let (mut t, w, _) = T::scene_over_w();
    t.key(KEY_LEFTMETA, true);
    t.button(BTN_MIDDLE, true);
    t.motion((30.0, 0.0));
    t.motion((-30.0, 0.0));
    let from = t.ls.probe.ptr_times.len();
    t.button(BTN_MIDDLE, false);
    let released = t.pointer_times_since(from, t.input_ms(), "the release");
    assert!(released > 0, "the release delivered no true position");
    t.key(KEY_LEFTMETA, false);

    // a popup with a grab under the pointer, then the output changes under it
    t.button(BTN_LEFT, true);
    t.button(BTN_LEFT, false);
    let qh = t.ls.qh.clone();
    let spec = PopupSpec {
        content: (60, 40),
        shadow: 0,
        anchor_rect: (15, 45, 1, 1),
        slide: false,
    };
    let p = t.ls.probe.popup_with_grab(&qh, "P", w, spec);
    t.adv();
    t.paint(p, GREEN);
    assert!(t.ls.state.keyboard_is_grabbed(), "the popup holds no grab");
    t.motion((1.0, 1.0));
    let from = t.ls.probe.ptr_times.len();
    let a = t.ls.output();
    t.ls.reconfigure(a, &output_k("A", (640, 480), 120, 144_000));
    t.adv();
    let grabbed = t.pointer_times_since(from, t.input_ms(), "under the grab");
    assert!(grabbed > 0, "nothing synthesised under the grab");
}

/// Invariant (INV-CAMERA-4; C5.2 "camera A is not touched ... a per-output render input, not a
/// camera"): a camera pan on a fullscreen output changes no pixel and causes no configure. It
/// moves the hidden camera, and that is what the output shows when the presentation ends.
#[test]
fn a_camera_pan_under_a_presentation() {
    let (mut t, w, _, _) = T::scene();
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    let camera = t.ls.state.core().cameras().center(0).expect("camera");
    let configures = t.configures(w).len();
    t.key(KEY_LEFTMETA, true);
    t.button(BTN_MIDDLE, true);
    assert_layers(&t.frame(), &[W_FULL], "pan started");
    t.motion((50.0, 30.0));
    assert_layers(&t.frame(), &[W_FULL], "during the pan");
    t.button(BTN_MIDDLE, false);
    t.key(KEY_LEFTMETA, false);
    assert_layers(&t.frame(), &[W_FULL], "after the pan");
    assert_eq!(
        t.configures(w).len(),
        configures,
        "a pan caused a configure"
    );
    let moved = t.ls.state.core().cameras().center(0).expect("camera");
    let (dx, dy) = (
        ((moved.x - camera.x) >> 16) as i32,
        ((moved.y - camera.y) >> 16) as i32,
    );
    assert_eq!((dx, dy), (-50, -30), "the hidden camera follows the hand");
    assert_eq!(
        t.ask(w, |tl| tl.unset_fullscreen()),
        vec![((200, 100), vec![ACTIVATED])]
    );
    t.resize(w, (200, 100), RED);
    // the content moved with the hand: every window 50 right and 30 down of where it was
    let shift = |((x0, y0, x1, y1), c): Layer| ((x0 - dx, y0 - dy, x1 - dx, y1 - dy), c);
    assert_layers(
        &t.frame(),
        &[shift(OTHER), shift(W_WINDOWED)],
        "the camera's view after the presentation",
    );
}

/// Invariant (INV-PRES-1 "focus loss does not end fullscreen"; a client request grants no
/// focus): a window mapped while another is presented does not appear on that output and does
/// not end the presentation.
#[test]
fn a_window_mapped_under_a_presentation_stays_hidden() {
    let (mut t, w, id, _) = T::scene();
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    let log = t.ls.probe.log.len();
    t.map("N", (100, 100), GREEN);
    assert_eq!(t.membership_since(log, "N"), Vec::<String>::new());
    assert_layers(&t.frame(), &[W_FULL], "still W alone");
    assert_eq!(t.ls.state.core().presentation(t.ls.output()), Some(id));
    assert_eq!(t.ls.state.core().fullscreen().len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------------------------

/// Invariant (INV-OUTPUT-1 / M9 "O disconnected → configure with the saved size"; M4 last
/// output): the only output goes away under a presentation. The client is told to leave with
/// its saved size, no state remains, and a reconnected output shows the camera's view.
#[test]
fn the_last_output_is_removed_under_a_presentation() {
    let (mut t, w, _, _) = T::scene();
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    let before = t.configures(w).len();
    let o = t.ls.output();
    t.ls.disconnect(o);
    t.adv();
    assert_eq!(
        t.configures(w)[before..].to_vec(),
        vec![((200, 100), vec![ACTIVATED])]
    );
    t.core_is_clean();
    t.resize(w, (200, 100), RED);
    let before = t.configures(w).len();
    let o = t.ls.connect(&output((640, 480), 120));
    t.ls.output = Some(o);
    t.adv();
    assert_eq!(
        t.configures(w)[before..].to_vec(),
        vec![],
        "a connect is not a toplevel event"
    );
    t.core_is_clean();
    assert_layers(&t.frame(), &[OTHER, W_WINDOWED], "the camera's view again");
}

/// Invariant (C5.2 "size = O's logical size"; M4 reconfiguration "one configure only for an
/// element in protocol fullscreen on this output"; mixed scales): on a 960x720 output at scale
/// 1.5 the fullscreen size is its logical 640x480; on a 1000x700 one the fractional logical
/// size 666.67x466.67 is offered rounded down. A scale change of the fullscreen output sends
/// one configure with the new size; a change of the other output sends none.
#[test]
fn fullscreen_on_outputs_of_different_scales() {
    let ls = Lockstep::with(
        PersistedCameras::default(),
        &[
            output_k("A", (640, 480), 120, 60_000),
            output_k("B", (960, 720), 180, 60_000),
        ],
    );
    let mut t = T::with(ls);
    let (w, id) = t.map("W", (200, 100), RED);
    t.focus(id);
    let (a, b) = {
        let o = t.ls.state.core().outputs();
        (o[0].id, o[1].id)
    };
    let wl_b = t.ls.probe.outputs[1].1.clone();
    assert_eq!(t.ask(w, |tl| tl.set_fullscreen(Some(&wl_b))), granted());
    assert_eq!(t.ls.state.core().presentation(b), Some(id));
    assert_eq!(t.ls.state.core().presentation(a), None);
    t.resize(w, (640, 480), RED);
    // A (scale 1) still shows W through its camera at its real size
    assert_layers(
        &t.frame_of(a, 0).1,
        &[((220, 190, 640, 480), RED)],
        "output A",
    );

    let before = t.configures(w).len();
    t.ls.reconfigure(b, &output_k("B", (1000, 700), 180, 60_000));
    t.adv();
    assert_eq!(
        t.configures(w)[before..].to_vec(),
        vec![((666, 466), vec![FULLSCREEN, ACTIVATED])],
        "the fractional logical size, rounded down"
    );
    let before = t.configures(w).len();
    t.ls.reconfigure(a, &output_k("A", (640, 480), 240, 60_000));
    t.adv();
    assert_eq!(
        t.configures(w)[before..].to_vec(),
        vec![],
        "another output's change is not this element's"
    );
    assert_eq!(t.ls.state.core().presentation(b), Some(id));
}

// ---------------------------------------------------------------------------------------------
// Audit finding A-3: a grant made before the first map takes the output at that map
// ---------------------------------------------------------------------------------------------

/// A-3: a toplevel that asks for fullscreen before its first map is granted, but takes the
/// output only when it maps. Until then the presented window keeps the output, its pixels and
/// its configure state, and the user keeps focus; a newcomer that never draws ends nothing.
/// When the newcomer draws, the presented window is told to leave with its saved size.
#[test]
fn a_pre_map_grant_takes_the_output_only_when_its_window_maps() {
    let (mut t, w, id, _) = T::scene();
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    let qh = t.ls.qh.clone();
    let x = t.ls.probe.toplevel_uncommitted(&qh, "X", (640, 480));
    // X does not draw yet: marked as configured so the probe does not paint it
    t.ls.probe.surfaces[x].configured = true;
    t.toplevel(x).set_fullscreen(None);
    let before = t.configures(w).len();
    t.ls.probe.surfaces[x].wl.commit();
    t.adv();
    t.adv();
    assert_eq!(
        t.configures(w)[before..].to_vec(),
        vec![],
        "W was told to leave"
    );
    assert_eq!(t.ls.state.core().presentation(t.ls.output()), Some(id));
    assert_eq!(t.ls.state.core().focus().target(), Some(id), "focus is W's");
    assert_layers(&t.frame(), &[W_FULL], "W still presented");

    // X draws: it takes the output, and W is offered its windowed size
    t.paint(x, GREEN);
    t.adv();
    assert_eq!(
        t.configures(w)[before..].to_vec(),
        vec![((200, 100), vec![ACTIVATED])],
        "W is told to leave when X maps"
    );
    let xid = t.ls.state.elements().last().expect("X");
    assert_eq!(t.ls.state.core().presentation(t.ls.output()), Some(xid));
    assert_layers(&t.frame(), &[((0, 0, 640, 480), GREEN)], "X presented");
}

/// A-8: keyboard focus does not move to a window the presentation hides. Super+N
/// (FocusNext) and a focus request leave it on the presented window, which keeps `activated`
/// and receives the keys; when the presentation ends, FocusNext reaches the other window again.
#[test]
fn focus_does_not_move_to_a_window_the_presentation_hides() {
    let (mut t, w, id, other) = T::scene();
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    let configures = t.configures(w).len();
    t.key(KEY_LEFTMETA, true);
    t.key(KEY_N, true);
    t.key(KEY_N, false);
    t.key(KEY_LEFTMETA, false);
    t.focus(other);
    assert_eq!(t.ls.state.core().focus().target(), Some(id));
    assert_eq!(t.ls.probe.kb.active.as_deref(), Some("W"));
    assert_eq!(t.configures(w).len(), configures, "W lost activated");
    t.key(KEY_A, true);
    t.key(KEY_A, false);
    assert_eq!(
        t.ls.probe.kb.active.as_deref(),
        Some("W"),
        "the keys went elsewhere"
    );

    t.ask(w, |tl| tl.unset_fullscreen());
    t.resize(w, (200, 100), RED);
    t.key(KEY_LEFTMETA, true);
    t.key(KEY_N, true);
    t.key(KEY_N, false);
    t.key(KEY_LEFTMETA, false);
    assert_eq!(t.ls.state.core().focus().target(), Some(other));
    assert_eq!(t.ls.probe.kb.active.as_deref(), Some("O"));
}

/// A-3: a newcomer granted before its first map that is destroyed without drawing ends nothing.
#[test]
fn a_pre_map_grant_that_never_draws_ends_nothing() {
    let (mut t, w, id, _) = T::scene();
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    let qh = t.ls.qh.clone();
    let x = t.ls.probe.toplevel_uncommitted(&qh, "X", (640, 480));
    t.ls.probe.surfaces[x].configured = true;
    t.toplevel(x).set_fullscreen(None);
    t.ls.probe.surfaces[x].wl.commit();
    t.adv();
    let before = t.configures(w).len();
    t.toplevel(x).destroy();
    t.ls.probe.surfaces[x].xdg.as_ref().expect("xdg").destroy();
    t.ls.probe.surfaces[x].wl.destroy();
    t.adv();
    assert_eq!(t.configures(w)[before..].to_vec(), vec![]);
    assert_eq!(t.ls.state.core().presentation(t.ls.output()), Some(id));
    assert_layers(&t.frame(), &[W_FULL], "W still presented");
}

/// INC-8 (M16, residual R-10, audit finding A-7): a presentation makes every other element on
/// its output unobserved. A client under a fullscreen window that draws from frame callbacks is
/// withheld, not starved: its callback completes once per fallback period P from when it became
/// pending, and at the first vblank once the presentation ends. (Replaces the INC-7 test that
/// recorded the dependency, `until_inc_8_a_window_hidden_by_a_presentation_gets_no_frame_callback`.)
#[test]
fn a_window_hidden_by_a_presentation_gets_the_fallback_and_re_entry() {
    let (mut t, w, _, other) = T::scene();
    let o = t.ls.probe.by_label("O").wl.clone();
    let o = t.ls.probe.surfaces.iter().position(|s| s.wl == o).unwrap();
    let frames = |t: &T| -> Vec<Duration> {
        t.ls.probe
            .frames
            .iter()
            .filter(|(_, l)| *l == "O")
            .map(|(at, _)| *at)
            .collect()
    };
    let qh = t.ls.qh.clone();
    // observed: the callback completes at the next vblank
    t.ls.probe.frame_and_draw(&qh, o);
    t.adv();
    t.adv();
    assert_eq!(frames(&t).len(), 1, "an observed element is clocked");
    t.ask(w, |tl| tl.set_fullscreen(None));
    assert!(
        !t.ls.state.core().membership().is_observed(other),
        "hidden by the presentation"
    );
    t.ls.probe.frame_and_draw(&qh, o);
    t.ls.pump();
    // the period runs from O's last completion, the vblank before the presentation
    let last = frames(&t)[0];
    let period = t.ls.state.core().frame_fallback_period();
    for _ in 0..30 {
        t.adv();
    }
    assert_eq!(
        frames(&t).len(),
        1,
        "withheld, not completed by vblanks (INV-FRAME-3)"
    );
    t.ls.run_until(last + period - Duration::from_millis(1));
    assert_eq!(frames(&t).len(), 1, "not before P");
    t.ls.run_until(last + period);
    assert_eq!(frames(&t)[1..], [last + period], "the fallback, once per P");
    t.ls.probe.frame_and_draw(&qh, o);
    t.adv();
    t.ask(w, |tl| tl.unset_fullscreen());
    t.adv();
    t.adv();
    assert_eq!(
        frames(&t).len(),
        3,
        "the callback completes once O is observed again (INV-FRAME-4)"
    );
}

// ---------------------------------------------------------------------------------------------
// Popups
// ---------------------------------------------------------------------------------------------

/// Invariant (M3 "position = get_unconstrained_geometry(target); target = anchor-output
/// viewport"; INV-COORD-2; OB-1 nested popups): a popup of a popup is constrained to the same
/// viewport as a popup of the toplevel. W's geometry spans physical x 220..420, so the viewport
/// ends at W-local x = 420. A 200-wide sliding popup anchored at W-local x = 240 must slide to
/// 220, whether its parent is W or a popup of W.
#[test]
fn a_nested_popup_is_constrained_like_a_first_level_one() {
    let (mut t, w, _, _) = T::scene();
    let qh = t.ls.qh.clone();
    let spec = |content, anchor_rect| PopupSpec {
        content,
        shadow: 0,
        anchor_rect,
        slide: true,
    };
    // control: first level, anchored at W-local (240, 30)
    let c =
        t.ls.probe
            .popup(&qh, "C", w, spec((200, 60), (240, 30, 1, 1)));
    t.adv();
    assert_eq!(
        t.ls.probe.surfaces[c].popup_configure,
        Some((220, 30, 200, 60)),
        "first level"
    );
    t.ls.probe.destroy_popup(c);
    t.adv();
    // P1 at W-local (150, 20); P2 anchored at P1-local (90, 10) = W-local (240, 30)
    let p1 =
        t.ls.probe
            .popup(&qh, "P1", w, spec((100, 60), (150, 20, 1, 1)));
    t.adv();
    assert_eq!(
        t.ls.probe.surfaces[p1].popup_configure,
        Some((150, 20, 100, 60))
    );
    let p2 =
        t.ls.probe
            .popup(&qh, "P2", p1, spec((200, 60), (90, 10, 1, 1)));
    t.adv();
    assert_eq!(
        t.ls.probe.surfaces[p2].popup_configure,
        Some((70, 10, 200, 60)),
        "nested: W-local 220 is P1-local 70"
    );
}

/// Invariants (C5.2 "popups: constraint boundary = the fullscreen output; rendered in the
/// presentation view"; INV-INPUT-10 ordered preemption; INV-PRES-1 "focus loss does not end
/// fullscreen"): a two-level grabbing menu of a fullscreen window is drawn over it at its place,
/// a sliding popup stays inside the fullscreen output, and a focus change dismisses the chain,
/// releases both grabs and leaves the presentation alone.
#[test]
fn a_menu_chain_of_a_fullscreen_window() {
    let (mut t, w, id, other) = T::scene();
    t.ask(w, |tl| tl.set_fullscreen(None));
    t.resize(w, (640, 480), RED);
    t.button(BTN_LEFT, true);
    let s1 = t.ls.probe.last_serial;
    t.button(BTN_LEFT, false);
    let qh = t.ls.qh.clone();
    let spec = |content, anchor_rect, slide| PopupSpec {
        content,
        shadow: 0,
        anchor_rect,
        slide,
    };
    let p1 =
        t.ls.probe
            .popup_with_grab_serial(&qh, "P1", w, spec((40, 20), (10, 10, 1, 1), false), s1);
    t.adv();
    t.paint(p1, GREEN);
    t.key(KEY_A, true);
    let s2 = t.ls.probe.last_serial;
    t.key(KEY_A, false);
    let p2 =
        t.ls.probe
            .popup_with_grab_serial(&qh, "P2", p1, spec((40, 20), (20, 10, 1, 1), false), s2);
    t.adv();
    t.paint(p2, YELLOW);
    assert!(t.ls.state.keyboard_is_grabbed() && t.ls.state.is_pointer_grabbed());
    assert_eq!(t.ls.probe.popup_dones, Vec::<&str>::new());
    // P1 at W-local (10,10); P2 at P1-local (20,10) = W-local (30,20); W's origin is the
    // output's origin
    assert_layers(
        &t.frame(),
        &[
            ((30, 20, 70, 40), YELLOW),
            ((10, 10, 50, 30), GREEN),
            W_FULL,
        ],
        "the menu over the fullscreen window",
    );
    // a sliding popup at the right edge stays on the fullscreen output: 640 - 40 = 600
    let e =
        t.ls.probe
            .popup(&qh, "E", w, spec((40, 20), (630, 100, 1, 1), true));
    t.adv();
    assert_eq!(
        t.ls.probe.surfaces[e].popup_configure,
        Some((600, 100, 40, 20))
    );
    t.ls.probe.destroy_popup(e);
    t.adv();

    let configures = t.configures(w).len();
    // A-8: O is hidden by the presentation, so focus does not move to it and the menu stays
    t.focus(other);
    assert_eq!(t.ls.state.core().focus().target(), Some(id));
    assert_eq!(t.ls.probe.popup_dones, Vec::<&str>::new());
    assert_eq!(t.configures(w).len(), configures);
    // focus loss (focus cleared, as by a click on no window)
    set_focus(&mut t.ls.state, &mut t.ls.input, None).expect("focus");
    t.adv();
    assert_eq!(t.ls.probe.popup_dones, vec!["P2", "P1"], "innermost first");
    assert!(!t.ls.state.keyboard_is_grabbed(), "keyboard grab left");
    assert!(!t.ls.state.is_pointer_grabbed(), "pointer grab left");
    assert_eq!(t.ls.state.core().presentation(t.ls.output()), Some(id));
    let got = t.configures(w)[configures..].to_vec();
    assert_eq!(
        got,
        vec![((640, 480), vec![FULLSCREEN])],
        "one configure: not activated, still fullscreen"
    );
    assert!(
        t.ls.probe.kb.violations.is_empty(),
        "{:?}",
        t.ls.probe.kb.violations
    );
}
