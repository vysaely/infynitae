//! Small protocols apps expect, on the production path with a real client: cursor-shape-v1.

mod common;

use common::input::*;
use common::{Lockstep, Probe, output};
use smithay::input::pointer::{CursorIcon, CursorImageStatus};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;

wayland_client::delegate_noop!(Probe: ignore WpCursorShapeManagerV1);
wayland_client::delegate_noop!(Probe: ignore WpCursorShapeDeviceV1);

/// Binds the global named `interface` (the probe saw every global at connection).
fn bind<I>(r: &Rig, interface: &str, version: u32) -> I
where
    I: wayland_client::Proxy + 'static,
    Probe: wayland_client::Dispatch<I, ()>,
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

/// A client that names a cursor shape gets that named cursor, drawn from the cursor theme like
/// any named cursor; a shape asked for with a stale serial is ignored.
#[test]
fn a_named_cursor_shape_becomes_the_named_cursor() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.map_w();
    r.motion((200.0, 200.0));
    r.motion((10.0, 0.0));
    let qh = r.ls.qh.clone();
    let manager: WpCursorShapeManagerV1 = bind(&r, "wp_cursor_shape_manager_v1", 1);
    let pointer = r.ls.probe.pointer.clone().expect("the probe has a pointer");
    let device = manager.get_pointer(&pointer, &qh, ());
    let serial = r.ls.probe.last_serial;
    device.set_shape(serial, Shape::Text);
    r.ls.pump();
    assert_eq!(
        r.ls.state.cursor_status(),
        &CursorImageStatus::Named(CursorIcon::Text)
    );
    // an old serial (not the latest enter) is not honoured
    device.set_shape(serial.wrapping_sub(1000), Shape::Pointer);
    r.ls.pump();
    assert_eq!(
        r.ls.state.cursor_status(),
        &CursorImageStatus::Named(CursorIcon::Text)
    );
}
