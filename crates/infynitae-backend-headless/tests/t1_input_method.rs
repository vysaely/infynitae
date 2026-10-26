//! Input methods with a real client playing the input method, the app and the lock screen:
//! text typed through the input method reaches the app, keys go to the input method's grab, and
//! while the session is locked nothing reaches or leaves through them: the keys go to the lock
//! surface past the grab, a virtual keyboard types nothing, the lock surface's text input never
//! activates the input method, and committed text goes nowhere.

mod common;

use std::io::Write;
use std::os::fd::AsFd;

use common::Probe;
use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::KEY_A;
use infynitae_wayland::convert;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, ExportMem, Offscreen};
use smithay::reexports::pixman::Image;
use smithay::utils::Rectangle;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_surface_v1::{
    self, ExtSessionLockSurfaceV1,
};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::ExtSessionLockV1;
use wayland_protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1;
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3;
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::{self, ZwpTextInputV3};
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_keyboard_grab_v2::{
    self, ZwpInputMethodKeyboardGrabV2,
};
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_manager_v2::ZwpInputMethodManagerV2;
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_v2::{
    self, ZwpInputMethodV2,
};
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

wayland_client::delegate_noop!(Probe: ignore ExtSessionLockManagerV1);
wayland_client::delegate_noop!(Probe: ignore ExtSessionLockV1);
wayland_client::delegate_noop!(Probe: ignore ZwpInputMethodManagerV2);
wayland_client::delegate_noop!(Probe: ignore ZwpTextInputManagerV3);
wayland_client::delegate_noop!(Probe: ignore ZwpVirtualKeyboardManagerV1);
wayland_client::delegate_noop!(Probe: ignore ZwpVirtualKeyboardV1);
wayland_client::delegate_noop!(Probe: ignore ZwpInputPopupSurfaceV2);
wayland_client::delegate_noop!(Probe: ignore WpSinglePixelBufferManagerV1);

impl Dispatch<ExtSessionLockSurfaceV1, ()> for Probe {
    fn event(
        st: &mut Self,
        s: &ExtSessionLockSurfaceV1,
        ev: ext_session_lock_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = ev
        {
            s.ack_configure(serial);
            st.log.push(format!("lock surface {width}x{height}"));
        }
    }
}

/// The input method's events: `im activate`, `im deactivate`, `im done`.
impl Dispatch<ZwpInputMethodV2, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpInputMethodV2,
        ev: zwp_input_method_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwp_input_method_v2::Event;
        st.log.push(match ev {
            Event::Activate => "im activate".into(),
            Event::Deactivate => "im deactivate".into(),
            Event::Done => "im done".into(),
            _ => return,
        });
    }
}

/// Keys the input method's grab receives: `grab key K`.
impl Dispatch<ZwpInputMethodKeyboardGrabV2, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpInputMethodKeyboardGrabV2,
        ev: zwp_input_method_keyboard_grab_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_input_method_keyboard_grab_v2::Event::Key { key, .. } = ev {
            st.log.push(format!("grab key {key}"));
        }
    }
}

/// The app's text input: `ti commit <text>`.
impl Dispatch<ZwpTextInputV3, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpTextInputV3,
        ev: zwp_text_input_v3::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_text_input_v3::Event::CommitString { text } = ev {
            st.log
                .push(format!("ti commit {}", text.unwrap_or_default()));
        }
    }
}

fn bind<I>(r: &Rig, interface: &str, version: u32) -> I
where
    I: wayland_client::Proxy + 'static,
    Probe: Dispatch<I, ()>,
{
    let (name, _, _) =
        r.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == interface)
            .cloned()
            .unwrap_or_else(|| panic!("{interface} is offered"));
    r.ls.registry().bind(name, version, &r.ls.qh, ())
}

fn key(r: &mut Rig, key: u32, pressed: bool) {
    r.t += 1000;
    if !r.ls.state.lock_key(key, pressed, r.t) {
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
    }
    r.ls.pump();
}

/// W focused, the input method bound; the lock enabled (not engaged).
fn setup() -> (Rig, ZwpInputMethodV2, WlSurface) {
    let mut r = Rig::x8();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let fx =
        r.ls.state
            .core_mut()
            .set_focus(&mut r.ls.input, Some(w))
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.ls.state.enable_session_lock();
    let manager: ZwpInputMethodManagerV2 = bind(&r, "zwp_input_method_manager_v2", 1);
    let seat = r.ls.probe.seat.clone().expect("seat");
    let im = manager.get_input_method(&seat, &r.ls.qh, ());
    r.ls.pump();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let surface = r.ls.probe.surfaces[w_idx].wl.clone();
    (r, im, surface)
}

/// The text input of whichever surface has the keyboard, enabled and committed.
fn enable_text_input(r: &mut Rig) -> ZwpTextInputV3 {
    let manager: ZwpTextInputManagerV3 = bind(r, "zwp_text_input_manager_v3", 1);
    let seat = r.ls.probe.seat.clone().expect("seat");
    let ti = manager.get_text_input(&seat, &r.ls.qh, ());
    r.ls.pump();
    ti.enable();
    ti.commit();
    r.ls.pump();
    ti
}

/// The input method commits `text` against the serial it has seen.
fn commit_text(r: &mut Rig, im: &ZwpInputMethodV2, text: &str) {
    let serial = r.ls.probe.log.iter().filter(|l| *l == "im done").count() as u32;
    im.commit_string(text.into());
    im.commit(serial);
    r.ls.pump();
}

/// Locks the session with a lock surface on the output, drawn, and types one key on it: the
/// keyboard goes to the lock surface with the first key, as when you start typing the password.
fn lock(r: &mut Rig) -> ExtSessionLockV1 {
    let manager: ExtSessionLockManagerV1 = bind(r, "ext_session_lock_manager_v1", 1);
    let qh = r.ls.qh.clone();
    let lock = manager.lock(&qh, ());
    let surface =
        r.ls.probe
            .compositor
            .as_ref()
            .expect("compositor")
            .create_surface(&qh, ());
    let output = r.ls.probe.outputs[0].1.clone();
    lock.get_lock_surface(&surface, &output, &qh, ());
    r.ls.pump();
    let size =
        r.ls.probe
            .log
            .iter()
            .rev()
            .find_map(|l| l.strip_prefix("lock surface ").map(str::to_owned))
            .expect("the lock surface was configured");
    let (w, h) = size.split_once('x').unwrap();
    let buffer =
        r.ls.probe
            .buffer(&qh, w.parse().unwrap(), h.parse().unwrap());
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    r.ls.pump();
    r.ls.state.lock_frame_drawn();
    r.ls.pump();
    assert!(r.ls.state.is_locked());
    key(r, KEY_A, true);
    key(r, KEY_A, false);
    let active = r.ls.probe.kb.active.clone().unwrap_or_default();
    assert!(
        active.starts_with('#'),
        "the lock surface has the keyboard, not {active}"
    );
    lock
}

#[test]
fn typed_text_reaches_the_app_and_keys_the_grab() {
    let (mut r, im, _) = setup();
    let _ti = enable_text_input(&mut r);
    assert!(
        r.ls.probe.log.contains(&"im activate".to_string()),
        "the app's text input activates the input method: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.retain(|l| l == "im done");
    commit_text(&mut r, &im, "あ");
    assert!(
        r.ls.probe.log.contains(&"ti commit あ".to_string()),
        "the app gets the text: {:?}",
        r.ls.probe.log
    );

    let _grab = im.grab_keyboard(&r.ls.qh, ());
    r.ls.pump();
    let before = r.ls.probe.key_events.len();
    key(&mut r, KEY_A, true);
    key(&mut r, KEY_A, false);
    assert!(
        r.ls.probe.log.contains(&format!("grab key {KEY_A}")),
        "the grab gets the key: {:?}",
        r.ls.probe.log
    );
    assert_eq!(
        r.ls.probe.key_events.len(),
        before,
        "and the app does not, the input method passes it on"
    );
}

#[test]
fn while_locked_the_grab_does_not_see_the_password() {
    let (mut r, im, _) = setup();
    let _grab = im.grab_keyboard(&r.ls.qh, ());
    r.ls.pump();
    let _lock = lock(&mut r);
    // and a grab taken during the lock
    let _grab2 = im.grab_keyboard(&r.ls.qh, ());
    r.ls.pump();
    r.ls.probe.log.clear();
    let before = r.ls.probe.key_events.len();
    key(&mut r, KEY_A, true);
    key(&mut r, KEY_A, false);
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("grab key")),
        "no key for the input method while locked: {:?}",
        r.ls.probe.log
    );
    assert!(
        r.ls.probe.key_events[before..].contains(&(KEY_A, true)),
        "the lock surface gets it"
    );
}

#[test]
fn while_locked_a_virtual_keyboard_types_nothing() {
    let (mut r, _im, _) = setup();
    let manager: ZwpVirtualKeyboardManagerV1 = bind(&r, "zwp_virtual_keyboard_manager_v1", 1);
    let seat = r.ls.probe.seat.clone().expect("seat");
    let vk = manager.create_virtual_keyboard(&seat, &r.ls.qh, ());
    let keymap = {
        use smithay::input::keyboard::xkb;
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let km = xkb::Keymap::new_from_names(&ctx, "", "", "us", "", None, xkb::COMPILE_NO_FLAGS)
            .expect("keymap");
        km.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1)
    };
    let path = std::env::temp_dir().join(format!("ifn-vk-{}", std::process::id()));
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(keymap.as_bytes()).unwrap();
    f.write_all(&[0]).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    vk.keymap(1, f.as_fd(), keymap.len() as u32 + 1);
    r.ls.pump();
    let _ = std::fs::remove_file(&path);

    // unlocked, it types into the focused window: the control for the locked case
    let before = r.ls.probe.key_events.len();
    vk.key(1, KEY_A, 1);
    vk.key(2, KEY_A, 0);
    r.ls.pump();
    assert!(
        r.ls.probe.key_events[before..].contains(&(KEY_A, true)),
        "unlocked, the virtual keyboard types"
    );

    let _lock = lock(&mut r);
    let before = r.ls.probe.key_events.len();
    vk.key(3, KEY_A, 1);
    vk.key(4, KEY_A, 0);
    r.ls.pump();
    assert_eq!(
        &r.ls.probe.key_events[before..],
        &[],
        "nothing reaches the lock surface from a virtual keyboard"
    );
}

#[test]
fn while_locked_the_input_method_neither_reads_nor_types() {
    let (mut r, im, _) = setup();
    let _lock = lock(&mut r);
    r.ls.probe.log.clear();
    // the lock surface (the keyboard's focus) enables a text input
    let _ti = enable_text_input(&mut r);
    assert!(
        !r.ls.probe.log.contains(&"im activate".to_string()),
        "the lock surface's text input does not activate the input method: {:?}",
        r.ls.probe.log
    );
    commit_text(&mut r, &im, "secret");
    assert!(
        !r.ls.probe.log.iter().any(|l| l.starts_with("ti commit")),
        "no text reaches the lock surface: {:?}",
        r.ls.probe.log
    );
}

/// The output rendered with Pixman, as ARGB pixels, and its width.
fn render(r: &mut Rig) -> (Vec<u32>, usize) {
    let o = r.ls.output();
    let mode = r.ls.state.core().output(o).expect("output").phys;
    let size = (mode.0 as i32, mode.1 as i32);
    let mut renderer = PixmanRenderer::new().expect("pixman");
    let mut image: Image<'static, 'static> = renderer
        .create_buffer(Fourcc::Argb8888, size.into())
        .expect("target");
    let mut target = renderer.bind(&mut image).expect("bind");
    convert::render_output(&mut r.ls.state, &mut renderer, &mut target, o, 0).expect("render");
    let m = renderer
        .copy_framebuffer(&target, Rectangle::from_size(size.into()), Fourcc::Argb8888)
        .expect("read back");
    let px = renderer
        .map_texture(&m)
        .expect("map")
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    (px, mode.0 as usize)
}

/// The candidate popup is drawn just under the text cursor of the window it types into.
#[test]
fn the_popup_is_drawn_under_the_text_cursor() {
    const COLOUR: u32 = 0xFF12_AB34;
    let (mut r, im, _) = setup();
    let ti = enable_text_input(&mut r);
    ti.set_cursor_rectangle(10, 20, 2, 16);
    ti.commit();
    r.ls.pump();

    let qh = r.ls.qh.clone();
    let popup =
        r.ls.probe
            .compositor
            .as_ref()
            .expect("compositor")
            .create_surface(&qh, ());
    let _role = im.get_input_popup_surface(&popup, &qh, ());
    let spb: WpSinglePixelBufferManagerV1 = bind(&r, "wp_single_pixel_buffer_manager_v1", 1);
    let ch = |v: u32| v * 0x0101_0101;
    let buffer = spb.create_u32_rgba_buffer(ch(0x12), ch(0xAB), ch(0x34), u32::MAX, &qh, ());
    let viewport =
        r.ls.probe
            .viewporter
            .as_ref()
            .expect("wp_viewporter")
            .get_viewport(&popup, &qh, ());
    viewport.set_destination(30, 20);
    popup.attach(Some(&buffer), 0, 0);
    popup.damage_buffer(0, 0, 1, 1);
    popup.commit();
    r.ls.pump();

    // where the window's surface is in the frame (W has no subsurface: its one entry)
    let o = r.ls.output();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let rect =
        r.ls.state
            .core_mut()
            .render_list(o)
            .expect("frame")
            .entries
            .iter()
            .find(|e| e.element == w)
            .map(|e| e.rect)
            .expect("the window is in the frame");

    let (px, width) = render(&mut r);
    let at: Vec<(usize, usize)> = px
        .iter()
        .enumerate()
        .filter(|(_, p)| **p == COLOUR)
        .map(|(i, _)| (i % width, i / width))
        .collect();
    assert_eq!(at.len(), 30 * 20, "the whole popup is drawn");
    let (x0, y0) = at.iter().fold((usize::MAX, usize::MAX), |(x, y), p| {
        (x.min(p.0), y.min(p.1))
    });
    assert_eq!(
        (x0 as i64, y0 as i64),
        (rect.x0 + 10, rect.y0 + 20 + 16),
        "its corner is at the cursor's bottom left"
    );
}

/// An input method that does not grab the keyboard (an on-screen keyboard: squeekboard): the
/// app with an enabled text input still gets the keyboard's keys, and the input method's text.
#[test]
fn an_input_method_without_a_grab_leaves_the_keys_to_the_app() {
    let (mut r, im, _) = setup();
    let _ti = enable_text_input(&mut r);
    assert!(r.ls.probe.log.contains(&"im activate".to_string()));
    let before = r.ls.probe.key_events.len();
    key(&mut r, KEY_A, true);
    key(&mut r, KEY_A, false);
    assert_eq!(
        r.ls.probe.key_events.len(),
        before + 2,
        "the app gets the key press and release: {:?}",
        r.ls.probe.log
    );
    r.ls.probe.log.retain(|l| l == "im done");
    commit_text(&mut r, &im, "b");
    assert!(r.ls.probe.log.contains(&"ti commit b".to_string()));
}
