//! Small protocols apps expect, on the production path with a real client: cursor-shape-v1,
//! xdg-foreign-v2, single-pixel-buffer-v1.

mod common;

use common::input::*;
use common::{Lockstep, Probe, output};
use infynitae_wayland::convert;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, ExportMem, Offscreen};
use smithay::input::pointer::{CursorIcon, CursorImageStatus};
use smithay::reexports::pixman::Image;
use smithay::utils::Rectangle;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use wayland_protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1;
use wayland_protocols::xdg::foreign::zv2::client::zxdg_exported_v2::{self, ZxdgExportedV2};
use wayland_protocols::xdg::foreign::zv2::client::zxdg_exporter_v2::ZxdgExporterV2;
use wayland_protocols::xdg::foreign::zv2::client::zxdg_imported_v2::ZxdgImportedV2;
use wayland_protocols::xdg::foreign::zv2::client::zxdg_importer_v2::ZxdgImporterV2;

wayland_client::delegate_noop!(Probe: ignore WpCursorShapeManagerV1);
wayland_client::delegate_noop!(Probe: ignore WpCursorShapeDeviceV1);
wayland_client::delegate_noop!(Probe: ignore ZxdgExporterV2);
wayland_client::delegate_noop!(Probe: ignore ZxdgImporterV2);
wayland_client::delegate_noop!(Probe: ignore ZxdgImportedV2);
wayland_client::delegate_noop!(Probe: ignore WpSinglePixelBufferManagerV1);

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

/// One frame of the production render path into a Pixman image of the output's size, as ARGB.
fn render(r: &mut Rig) -> Vec<u32> {
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
    renderer
        .map_texture(&m)
        .expect("map")
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect()
}

/// A one-pixel buffer stretched by a viewport draws its colour over the whole destination: a
/// 50 x 50 subsurface of one opaque colour no other surface here uses.
#[test]
fn a_single_pixel_buffer_draws_its_colour() {
    let mut r = Rig::x8();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let qh = r.ls.qh.clone();
    let manager: WpSinglePixelBufferManagerV1 = bind(&r, "wp_single_pixel_buffer_manager_v1", 1);
    let s_idx =
        r.ls.probe
            .subsurface_uncommitted(&qh, "S", w_idx, (10, 10), (50, 50), false);
    // 0x12, 0xAB, 0x34, opaque: each channel scaled to the full u32 range
    let ch = |v: u32| v * 0x0101_0101;
    let buffer = manager.create_u32_rgba_buffer(ch(0x12), ch(0xAB), ch(0x34), u32::MAX, &qh, ());
    let s = r.ls.probe.surfaces[s_idx].wl.clone();
    let viewport =
        r.ls.probe
            .viewporter
            .as_ref()
            .expect("wp_viewporter")
            .get_viewport(&s, &qh, ());
    viewport.set_destination(50, 50);
    s.attach(Some(&buffer), 0, 0);
    s.damage_buffer(0, 0, 1, 1);
    s.commit();
    r.ls.probe.surfaces[w_idx].wl.commit();
    r.ls.pump();
    r.frame();
    let colour = 0xFF12_AB34u32;
    let count = render(&mut r).iter().filter(|p| **p == colour).count();
    assert_eq!(count, 50 * 50, "pixels of the single-pixel colour");
}
