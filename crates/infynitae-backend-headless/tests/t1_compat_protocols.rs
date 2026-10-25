//! Small protocols apps expect, on the production path with a real client: cursor-shape-v1,
//! xdg-foreign-v2, single-pixel-buffer-v1, alpha-modifier-v1, content-type-v1, xdg-system-bell-v1,
//! ext-foreign-toplevel-list-v1.

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
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::{
    self, ExtForeignToplevelHandleV1,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::{
    self, ExtForeignToplevelListV1,
};
use wayland_protocols::wp::alpha_modifier::v1::client::wp_alpha_modifier_surface_v1::WpAlphaModifierSurfaceV1;
use wayland_protocols::wp::alpha_modifier::v1::client::wp_alpha_modifier_v1::WpAlphaModifierV1;
use wayland_protocols::wp::content_type::v1::client::wp_content_type_manager_v1::WpContentTypeManagerV1;
use wayland_protocols::wp::content_type::v1::client::wp_content_type_v1::{self, WpContentTypeV1};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use wayland_protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1;
use wayland_protocols::xdg::foreign::zv2::client::zxdg_exported_v2::{self, ZxdgExportedV2};
use wayland_protocols::xdg::foreign::zv2::client::zxdg_exporter_v2::ZxdgExporterV2;
use wayland_protocols::xdg::foreign::zv2::client::zxdg_imported_v2::ZxdgImportedV2;
use wayland_protocols::xdg::foreign::zv2::client::zxdg_importer_v2::ZxdgImporterV2;
use wayland_protocols::xdg::system_bell::v1::client::xdg_system_bell_v1::XdgSystemBellV1;

wayland_client::delegate_noop!(Probe: ignore WpCursorShapeManagerV1);
wayland_client::delegate_noop!(Probe: ignore WpCursorShapeDeviceV1);
wayland_client::delegate_noop!(Probe: ignore ZxdgExporterV2);
wayland_client::delegate_noop!(Probe: ignore ZxdgImporterV2);
wayland_client::delegate_noop!(Probe: ignore ZxdgImportedV2);
wayland_client::delegate_noop!(Probe: ignore WpSinglePixelBufferManagerV1);
wayland_client::delegate_noop!(Probe: ignore WpAlphaModifierV1);
wayland_client::delegate_noop!(Probe: ignore WpAlphaModifierSurfaceV1);
wayland_client::delegate_noop!(Probe: ignore WpContentTypeManagerV1);
wayland_client::delegate_noop!(Probe: ignore WpContentTypeV1);
wayland_client::delegate_noop!(Probe: ignore XdgSystemBellV1);

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

/// The window list goes to the probe's log: `listed`, `title <t>`, `app_id <a>`, `done`, `closed`.
impl wayland_client::Dispatch<ExtForeignToplevelListV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ExtForeignToplevelListV1,
        ev: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { .. } = ev {
            st.log.push("listed".into());
        }
    }

    wayland_client::event_created_child!(Probe, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl wayland_client::Dispatch<ExtForeignToplevelHandleV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ExtForeignToplevelHandleV1,
        ev: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        use ext_foreign_toplevel_handle_v1::Event;
        st.log.push(match ev {
            Event::Title { title } => format!("title {title}"),
            Event::AppId { app_id } => format!("app_id {app_id}"),
            Event::Done => "done".into(),
            Event::Closed => "closed".into(),
            _ => return,
        });
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

/// The opaque colour of the test square, as rendered ARGB; no other surface here uses it.
const SQUARE: u32 = 0xFF12_AB34;

/// A 50 x 50 subsurface of W at (10, 10): one pixel of [`SQUARE`] stretched by a viewport. Its
/// surface is returned uncommitted with the buffer attached, so a test can add state first.
fn square(r: &mut Rig) -> (usize, wayland_client::protocol::wl_surface::WlSurface) {
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let qh = r.ls.qh.clone();
    let manager: WpSinglePixelBufferManagerV1 = bind(r, "wp_single_pixel_buffer_manager_v1", 1);
    let s_idx =
        r.ls.probe
            .subsurface_uncommitted(&qh, "S", w_idx, (10, 10), (50, 50), false);
    // each channel scaled to the full u32 range
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
    (w_idx, s)
}

/// Commits the square and its parent W, and renders.
fn show(
    r: &mut Rig,
    w_idx: usize,
    s: &wayland_client::protocol::wl_surface::WlSurface,
) -> Vec<u32> {
    s.commit();
    r.ls.probe.surfaces[w_idx].wl.commit();
    r.ls.pump();
    r.frame();
    render(r)
}

/// A one-pixel buffer stretched by a viewport draws its colour over the whole destination.
#[test]
fn a_single_pixel_buffer_draws_its_colour() {
    let mut r = Rig::x8();
    let (w_idx, s) = square(&mut r);
    let px = show(&mut r, w_idx, &s);
    assert_eq!(px.iter().filter(|p| **p == SQUARE).count(), 50 * 50);
}

/// An alpha multiplier of 0 makes the square invisible; 1 draws it as before.
#[test]
fn the_alpha_modifier_applies_to_the_surface() {
    let mut r = Rig::x8();
    let (w_idx, s) = square(&mut r);
    let qh = r.ls.qh.clone();
    let modifier: WpAlphaModifierV1 = bind(&r, "wp_alpha_modifier_v1", 1);
    let alpha = modifier.get_surface(&s, &qh, ());
    alpha.set_multiplier(0);
    let px = show(&mut r, w_idx, &s);
    assert_eq!(
        px.iter().filter(|p| **p == SQUARE).count(),
        0,
        "invisible at 0"
    );
    alpha.set_multiplier(u32::MAX);
    let px = show(&mut r, w_idx, &s);
    assert_eq!(
        px.iter().filter(|p| **p == SQUARE).count(),
        50 * 50,
        "opaque at 1"
    );
}

/// A window's content type is what its client said.
#[test]
fn the_content_type_is_kept() {
    use smithay::reexports::wayland_protocols::wp::content_type::v1::server::wp_content_type_v1::Type;
    let mut r = Rig::x8();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    assert_eq!(r.ls.state.content_type(w), Type::None);
    let qh = r.ls.qh.clone();
    let manager: WpContentTypeManagerV1 = bind(&r, "wp_content_type_manager_v1", 1);
    let surface = r.ls.probe.surfaces[w_idx].wl.clone();
    let ct = manager.get_surface_content_type(&surface, &qh, ());
    ct.set_content_type(wp_content_type_v1::Type::Game);
    surface.commit();
    r.ls.pump();
    assert_eq!(r.ls.state.content_type(w), Type::Game);
}

/// Ringing the bell is accepted: the client stays connected and keeps working.
#[test]
fn the_bell_rings_quietly() {
    let mut r = Rig::x8();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let bell: XdgSystemBellV1 = bind(&r, "xdg_system_bell_v1", 1);
    let surface = r.ls.probe.surfaces[w_idx].wl.clone();
    bell.ring(Some(&surface));
    bell.ring(None);
    r.ls.pump();
    // still served: a new surface after the bell is drawn
    let (w_idx2, s) = square(&mut r);
    assert_eq!(w_idx2, w_idx);
    let px = show(&mut r, w_idx, &s);
    assert_eq!(px.iter().filter(|p| **p == SQUARE).count(), 50 * 50);
}

/// A bar sees the open window with its title and app id, follows a title change, and sees the
/// window close.
#[test]
fn the_window_list_follows_the_windows() {
    let mut r = Rig::x8();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let toplevel = r.ls.probe.surfaces[w_idx].toplevel.clone().unwrap();
    toplevel.set_app_id("org.example.W".into());
    toplevel.set_title("first".into());
    r.ls.pump();
    r.ls.probe.log.clear();
    let _list: ExtForeignToplevelListV1 = bind(&r, "ext_foreign_toplevel_list_v1", 1);
    r.ls.pump();
    let log = std::mem::take(&mut r.ls.probe.log);
    assert!(log.contains(&"listed".to_string()), "{log:?}");
    assert!(log.contains(&"title first".to_string()), "{log:?}");
    assert!(log.contains(&"app_id org.example.W".to_string()), "{log:?}");
    assert_eq!(log.last().map(String::as_str), Some("done"), "{log:?}");

    toplevel.set_title("second".into());
    r.ls.pump();
    let log = std::mem::take(&mut r.ls.probe.log);
    assert_eq!(log, ["title second", "done"], "a title change");

    toplevel.destroy();
    r.ls.pump();
    let log = std::mem::take(&mut r.ls.probe.log);
    assert_eq!(log, ["closed"], "the window closed");
}
