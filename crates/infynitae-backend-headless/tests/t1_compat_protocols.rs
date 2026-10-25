//! Small protocols apps expect, on the production path with a real client: cursor-shape-v1,
//! xdg-foreign-v2.

mod common;

use common::input::*;
use common::{Lockstep, Probe, output};
use smithay::input::pointer::{CursorIcon, CursorImageStatus};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use wayland_protocols::xdg::foreign::zv2::client::zxdg_exported_v2::{self, ZxdgExportedV2};
use wayland_protocols::xdg::foreign::zv2::client::zxdg_exporter_v2::ZxdgExporterV2;
use wayland_protocols::xdg::foreign::zv2::client::zxdg_imported_v2::ZxdgImportedV2;
use wayland_protocols::xdg::foreign::zv2::client::zxdg_importer_v2::ZxdgImporterV2;

wayland_client::delegate_noop!(Probe: ignore WpCursorShapeManagerV1);
wayland_client::delegate_noop!(Probe: ignore WpCursorShapeDeviceV1);
wayland_client::delegate_noop!(Probe: ignore ZxdgExporterV2);
wayland_client::delegate_noop!(Probe: ignore ZxdgImporterV2);
wayland_client::delegate_noop!(Probe: ignore ZxdgImportedV2);

/// The handle of an exported toplevel goes to the probe's log as `exported <handle>`.
impl wayland_client::Dispatch<ZxdgExportedV2, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZxdgExportedV2,
        ev: zxdg_exported_v2::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        if let zxdg_exported_v2::Event::Handle { handle } = ev {
            st.log.push(format!("exported {handle}"));
        }
    }
}

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

/// A window exported by one toplevel and imported by another becomes that one's parent, as
/// `xdg_toplevel.set_parent` makes it (a portal dialog over the app that opened it).
#[test]
fn an_imported_toplevel_becomes_the_parent() {
    let mut r = Rig::x8();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let qh = r.ls.qh.clone();
    let exporter: ZxdgExporterV2 = bind(&r, "zxdg_exporter_v2", 1);
    let w_surface = r.ls.probe.surfaces[w_idx].wl.clone();
    let _exported = exporter.export_toplevel(&w_surface, &qh, ());
    r.ls.pump();
    let handle =
        r.ls.probe
            .log
            .iter()
            .find_map(|l| l.strip_prefix("exported ").map(str::to_owned))
            .expect("the export got a handle");

    let d_idx = r.ls.probe.toplevel(&qh, "D", (300, 200), 0);
    r.ls.pump();
    r.frame();
    let d = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    assert_eq!(r.ls.state.core().toplevel_parent(d), None);

    let importer: ZxdgImporterV2 = bind(&r, "zxdg_importer_v2", 1);
    let imported = importer.import_toplevel(handle, &qh, ());
    let d_surface = r.ls.probe.surfaces[d_idx].wl.clone();
    imported.set_parent_of(&d_surface);
    d_surface.commit();
    r.ls.pump();
    assert_eq!(r.ls.state.core().toplevel_parent(d), Some(w));
}
