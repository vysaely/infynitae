//! Virtual input (`zwlr_virtual_pointer_v1`, a virtual keyboard with the seat's keymap) with a
//! real client, as the remote desktop portal uses it: relative and absolute motion move the
//! pointer, a click reaches the window under it, scrolling waits for `frame`, a button or key
//! held when the device goes away is released, and the keyboard's Super with the pointer's drag
//! moves a window like a real Super+drag. While locked the pointer goes nowhere. A virtual
//! keyboard with another keymap keeps smithay's path (it types into the window, no shortcuts).

mod common;

use std::io::Write;
use std::os::fd::AsFd;

use common::Probe;
use common::input::*;
use infynitae_core::pointer::PointerState;
use wayland_client::protocol::wl_pointer::{Axis, ButtonState};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_surface_v1::{
    self, ExtSessionLockSurfaceV1,
};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::ExtSessionLockV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

wayland_client::delegate_noop!(Probe: ignore ZwlrVirtualPointerManagerV1);
wayland_client::delegate_noop!(Probe: ignore ZwlrVirtualPointerV1);
wayland_client::delegate_noop!(Probe: ignore ZwpVirtualKeyboardManagerV1);
wayland_client::delegate_noop!(Probe: ignore ZwpVirtualKeyboardV1);
wayland_client::delegate_noop!(Probe: ignore ExtSessionLockManagerV1);
wayland_client::delegate_noop!(Probe: ignore ExtSessionLockV1);

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

const BTN_LEFT: u32 = 0x110;
const KEY_LEFTMETA: u32 = 125;
const KEY_A: u32 = 30;

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

/// A virtual pointer on the rig's one screen.
fn pointer(r: &mut Rig) -> ZwlrVirtualPointerV1 {
    let manager: ZwlrVirtualPointerManagerV1 = bind(r, "zwlr_virtual_pointer_manager_v1", 2);
    let seat = r.ls.probe.seat.clone();
    let output = r.ls.probe.outputs[0].1.clone();
    let vp = manager.create_virtual_pointer_with_output(seat.as_ref(), Some(&output), &r.ls.qh, ());
    r.ls.pump();
    vp
}

/// A virtual keyboard that uploaded the seat's keymap as `wl_keyboard` sent it (`None`), as the
/// portal does, or the keymap `layout` compiles to.
fn keyboard(r: &mut Rig, layout: Option<&str>) -> ZwpVirtualKeyboardV1 {
    let manager: ZwpVirtualKeyboardManagerV1 = bind(r, "zwp_virtual_keyboard_manager_v1", 1);
    let seat = r.ls.probe.seat.clone().expect("a seat");
    let vk = manager.create_virtual_keyboard(&seat, &r.ls.qh, ());
    let keymap = match layout {
        None => {
            r.ls.probe
                .keymap
                .clone()
                .expect("the seat's keymap was sent")
        }
        Some(layout) => {
            use smithay::input::keyboard::xkb;
            let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
            let mut text =
                xkb::Keymap::new_from_names(&ctx, "", "", layout, "", None, xkb::COMPILE_NO_FLAGS)
                    .expect("keymap")
                    .get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1)
                    .into_bytes();
            text.push(0);
            text
        }
    };
    // one file per call: tests run in parallel, each removes its own
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("ifn-vi-{}-{n}-{layout:?}", std::process::id()));
    std::fs::File::create(&path)
        .unwrap()
        .write_all(&keymap)
        .unwrap();
    let f = std::fs::File::open(&path).unwrap();
    vk.keymap(1, f.as_fd(), keymap.len() as u32);
    r.ls.pump();
    let _ = std::fs::remove_file(&path);
    vk
}

fn at(r: &Rig) -> (f64, f64) {
    match r.ls.state.core().pointer() {
        PointerState::Located { p, .. } => p,
        PointerState::NoOutput => panic!("the pointer is on a screen"),
    }
}

fn absolute(r: &mut Rig, vp: &ZwlrVirtualPointerV1, p: (f64, f64)) {
    vp.motion_absolute(0, p.0 as u32, p.1 as u32, 1920, 1080);
    vp.frame();
    r.ls.pump();
}

/// A point well inside W, output logical.
fn on_w(r: &mut Rig) -> (f64, f64) {
    let output = r.ls.output();
    for y in (0..1080).step_by(40) {
        for x in (0..1920).step_by(40) {
            let pt = infynitae_core::geom::OutputLogicalPoint {
                x: f64::from(x) + 40.0,
                y: f64::from(y) + 40.0,
            };
            if r.ls.state.core_mut().hit_test(output, pt).is_some() {
                return (pt.x, pt.y);
            }
        }
    }
    panic!("W is on the screen");
}

#[test]
fn relative_and_absolute_motion_move_the_pointer() {
    let mut r = Rig::x8();
    let vp = pointer(&mut r);
    let p0 = at(&r);
    vp.motion(0, 30.0, -20.0);
    vp.frame();
    r.ls.pump();
    let p = at(&r);
    assert!(
        (p.0 - (p0.0 + 30.0)).abs() < 1e-6 && (p.1 - (p0.1 - 20.0)).abs() < 1e-6,
        "{p0:?} -> {p:?}"
    );
    absolute(&mut r, &vp, (960.0, 540.0));
    assert_eq!(at(&r), (960.0, 540.0), "the middle of its screen");
}

#[test]
fn a_click_reaches_the_window_under_it() {
    let mut r = Rig::x8();
    let vp = pointer(&mut r);
    let p = on_w(&mut r);
    absolute(&mut r, &vp, p);
    r.ls.probe.ptr.clear();
    vp.button(0, BTN_LEFT, ButtonState::Pressed);
    vp.button(0, BTN_LEFT, ButtonState::Released);
    vp.frame();
    r.ls.pump();
    let ptr = &r.ls.probe.ptr;
    assert!(
        ptr.iter()
            .any(|l| l == &format!("button {BTN_LEFT} pressed"))
            && ptr
                .iter()
                .any(|l| l == &format!("button {BTN_LEFT} released")),
        "{ptr:?}"
    );
}

#[test]
fn scrolling_waits_for_frame() {
    let mut r = Rig::x8();
    let vp = pointer(&mut r);
    let p = on_w(&mut r);
    absolute(&mut r, &vp, p);
    r.ls.probe.ptr.clear();
    vp.axis(0, Axis::VerticalScroll, 15.0);
    r.ls.pump();
    assert!(
        !r.ls.probe.ptr.iter().any(|l| l.starts_with("axis")),
        "{:?}",
        r.ls.probe.ptr
    );
    vp.frame();
    r.ls.pump();
    assert!(
        r.ls.probe.ptr.iter().any(|l| l.starts_with("axis")),
        "{:?}",
        r.ls.probe.ptr
    );
}

#[test]
fn a_button_held_is_released_when_the_pointer_goes_away() {
    let mut r = Rig::x8();
    let vp = pointer(&mut r);
    let p = on_w(&mut r);
    absolute(&mut r, &vp, p);
    vp.button(0, BTN_LEFT, ButtonState::Pressed);
    vp.frame();
    r.ls.pump();
    assert!(!r.ls.state.core().buttons().pressed().is_empty());
    vp.destroy();
    r.ls.pump();
    assert!(
        r.ls.state.core().buttons().pressed().is_empty(),
        "released with it"
    );
}

/// The remote desktop case: the keyboard's Super and the pointer's drag move W, as a real
/// Super+drag does, and W sees no button.
#[test]
fn super_on_the_keyboard_and_a_drag_on_the_pointer_move_a_window() {
    let mut r = Rig::x8();
    let vk = keyboard(&mut r, None);
    let vp = pointer(&mut r);
    let w = r.ls.state.core().stage().mapped_elements()[0];
    let before = r.ls.state.core().stage().anchor(w).expect("W's anchor");
    let p = on_w(&mut r);
    absolute(&mut r, &vp, p);
    vk.key(0, KEY_LEFTMETA, 1);
    r.ls.pump();
    assert!(r.ls.state.seat_modifiers().logo, "Super reached the seat");
    r.ls.probe.ptr.clear();
    vp.button(0, BTN_LEFT, ButtonState::Pressed);
    vp.frame();
    vp.motion(0, 100.0, 50.0);
    vp.frame();
    vp.button(0, BTN_LEFT, ButtonState::Released);
    vp.frame();
    vk.key(0, KEY_LEFTMETA, 0);
    r.ls.pump();
    let after = r.ls.state.core().stage().anchor(w).expect("W's anchor");
    assert_ne!(before, after, "W moved");
    assert!(
        !r.ls.probe.ptr.iter().any(|l| l.starts_with("button")),
        "the drag was the compositor's: {:?}",
        r.ls.probe.ptr
    );
    assert!(!r.ls.state.seat_modifiers().logo);
}

#[test]
fn a_key_held_is_released_when_the_keyboard_goes_away() {
    let mut r = Rig::x8();
    let vk = keyboard(&mut r, None);
    vk.key(0, KEY_LEFTMETA, 1);
    r.ls.pump();
    assert!(r.ls.state.seat_modifiers().logo);
    vk.destroy();
    r.ls.pump();
    assert!(!r.ls.state.seat_modifiers().logo, "released with it");
}

/// Another keymap (wtype, an input method's own): smithay types it into the focused window, and
/// the seat (the shortcuts) never sees it.
#[test]
fn a_keyboard_with_another_keymap_types_without_the_shortcuts() {
    let mut r = Rig::x8();
    let vk = keyboard(&mut r, Some("de"));
    vk.key(0, KEY_LEFTMETA, 1);
    r.ls.pump();
    assert!(!r.ls.state.seat_modifiers().logo, "not the seat's key path");
    vk.key(0, KEY_LEFTMETA, 0);
    r.ls.pump();
}

/// The screen locked, its lock surface drawn.
fn lock(r: &mut Rig) -> ExtSessionLockV1 {
    r.ls.state.enable_session_lock();
    r.ls.pump();
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
    lock
}

/// While locked the pointer goes nowhere and the keyboard types nothing (only your own keys
/// reach the lock screen), but a key it held before is still released.
#[test]
fn while_locked_the_pointer_goes_nowhere_and_the_keyboard_types_nothing() {
    let mut r = Rig::x8();
    let vk = keyboard(&mut r, None);
    let vp = pointer(&mut r);
    vk.key(0, KEY_LEFTMETA, 1);
    r.ls.pump();
    assert!(r.ls.state.seat_modifiers().logo);
    let _lock = lock(&mut r);
    let p0 = at(&r);
    vp.motion(0, 50.0, 50.0);
    vp.frame();
    r.ls.pump();
    assert_eq!(at(&r), p0, "the pointer did not move");
    let before = r.ls.probe.key_events.len();
    vk.key(0, KEY_A, 1);
    vk.key(0, KEY_A, 0);
    r.ls.pump();
    assert!(
        !r.ls.probe.key_events[before..].contains(&(KEY_A, true)),
        "nothing typed into the lock screen: {:?}",
        &r.ls.probe.key_events[before..]
    );
    vk.key(0, KEY_LEFTMETA, 0);
    r.ls.pump();
    assert!(
        !r.ls.state.seat_modifiers().logo,
        "the held Super was released"
    );
}
