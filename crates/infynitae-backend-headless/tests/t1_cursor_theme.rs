//! Production path tests for Xcursor themes: configured themes are loaded and drawn
//! as the compositor's own pointer for named cursors and the empty canvas, sharp at
//! output scale, with fallback to the built-in triangle, and theme changes apply
//! on configuration reload.

mod common;

use common::{Lockstep, output};
use infynitae_backend_headless::inject;
use infynitae_core::PhysicalEvent;
use infynitae_wayland::convert;
use infynitae_wayland::cursor::{TestCursorImageDesc, TestTempDir, make_xcursor_file};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, ExportMem, Offscreen};
use smithay::reexports::pixman::Image;
use smithay::utils::{Physical, Rectangle};

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

const MAGENTA: u32 = 0xFFFF_00FF;
const CYAN: u32 = 0xFF00_FFFF;
const BLUE: u32 = 0xFF00_00FF;
const BLACK: u32 = 0xFF00_0000;
const WHITE: u32 = 0xFFFF_FFFF;

fn write_cursor_theme(
    icons_dir: &std::path::Path,
    theme_name: &str,
    cursor_name: &str,
    images: &[TestCursorImageDesc<'_>],
) {
    let cursors_dir = icons_dir.join(theme_name).join("cursors");
    std::fs::create_dir_all(&cursors_dir).expect("create cursors dir");
    let data = make_xcursor_file(images);
    std::fs::write(cursors_dir.join(cursor_name), data).expect("write cursor file");
}

fn write_config(config_home: &std::path::Path, theme: &str, size: u32) {
    let cfg_dir = config_home.join(".config/infynitae");
    std::fs::create_dir_all(&cfg_dir).expect("create config dir");
    let content = format!("CURSOR_THEME=\"{theme}\"\nCURSOR_SIZE=\"{size}\"\n");
    std::fs::write(cfg_dir.join("config"), content).expect("write config");
}

#[test]
fn the_console_target_draws_the_theme_cursor_at_the_pointer() {
    let temp = TestTempDir::new("t1_cursor_theme_draw");
    let icons_dir = temp.path().join("icons");

    // 4x4 magenta square with hotspot at (1, 1)
    let magenta_pixels = [0xFF, 0x00, 0xFF, 0xFF].repeat(4 * 4);
    write_cursor_theme(
        &icons_dir,
        "test_theme",
        "default",
        &[(24, 4, 4, (1, 1), &magenta_pixels)],
    );
    write_config(temp.path(), "test_theme", 24);

    let mut ls = Lockstep::new(Some(output((640, 480), 120)));
    ls.state.set_console_target();
    ls.state.set_config_home(temp.path().to_path_buf());
    ls.state
        .set_xcursor_path(icons_dir.to_string_lossy().into_owned());
    ls.state.reload_config();

    nudge(&mut ls, (10.0, 0.0));
    let mut screen = Screen::new((640, 480));
    let (_, pixels) = screen.frame(&mut ls, 0);
    let at = |x: usize, y: usize| pixels[y * 640 + x];

    let canvas = at(0, 0);
    assert_ne!(canvas, MAGENTA, "canvas background is not magenta");

    // Hotspot is (1, 1) and pointer is at (330, 240), so top-left is (329, 239)
    assert_eq!(at(330, 240), MAGENTA, "pointer pixel is theme image");
    assert_eq!(at(329, 239), MAGENTA, "top-left pixel is theme image");
    assert_eq!(at(332, 242), MAGENTA, "bottom-right pixel is theme image");
    assert_eq!(at(328, 239), canvas, "outside left is canvas");
    assert_eq!(at(333, 240), canvas, "outside right is canvas");
}

#[test]
fn theme_change_applies_on_config_reload() {
    let temp = TestTempDir::new("t1_cursor_reload");
    let icons_dir = temp.path().join("icons");

    let magenta_pixels = [0xFF, 0x00, 0xFF, 0xFF].repeat(4 * 4);
    write_cursor_theme(
        &icons_dir,
        "theme_magenta",
        "default",
        &[(24, 4, 4, (1, 1), &magenta_pixels)],
    );

    let cyan_pixels = [0xFF, 0xFF, 0x00, 0xFF].repeat(4 * 4);
    write_cursor_theme(
        &icons_dir,
        "theme_cyan",
        "default",
        &[(24, 4, 4, (1, 1), &cyan_pixels)],
    );

    write_config(temp.path(), "theme_magenta", 24);

    let mut ls = Lockstep::new(Some(output((640, 480), 120)));
    ls.state.set_console_target();
    ls.state.set_config_home(temp.path().to_path_buf());
    ls.state
        .set_xcursor_path(icons_dir.to_string_lossy().into_owned());
    ls.state.reload_config();

    nudge(&mut ls, (10.0, 0.0));
    let mut screen = Screen::new((640, 480));
    let (_, pixels) = screen.frame(&mut ls, 0);
    assert_eq!(pixels[240 * 640 + 330], MAGENTA, "initially magenta");

    // Change configuration to theme_cyan
    write_config(temp.path(), "theme_cyan", 24);
    ls.state.reload_config();

    let (_, pixels) = screen.frame(&mut ls, 0);
    assert_eq!(pixels[240 * 640 + 330], CYAN, "reloaded to cyan theme");
}

#[test]
fn fallback_to_triangle_when_theme_does_not_exist() {
    let temp = TestTempDir::new("t1_cursor_fallback");
    write_config(temp.path(), "nonexistent_theme", 24);

    let mut ls = Lockstep::new(Some(output((640, 480), 120)));
    ls.state.set_console_target();
    ls.state.set_config_home(temp.path().to_path_buf());
    ls.state.set_xcursor_path(
        temp.path()
            .join("empty_icons")
            .to_string_lossy()
            .into_owned(),
    );
    ls.state.reload_config();

    nudge(&mut ls, (10.0, 0.0));
    let mut screen = Screen::new((640, 480));
    let (_, pixels) = screen.frame(&mut ls, 0);
    let at = |x: usize, y: usize| pixels[y * 640 + x];

    let canvas = at(0, 0);
    assert_eq!(at(330, 240), BLACK, "the tip, on the pointer's pixel");
    assert_eq!(at(329, 240), canvas, "nothing left of the tip");
    assert_eq!(at(330, 245), BLACK, "row 5's outline, left column");
    assert_eq!(at(331, 245), WHITE, "row 5's fill");
    assert_eq!(at(334, 245), WHITE, "row 5's fill ends at 330 + 5");
    assert_eq!(at(335, 245), BLACK, "row 5's outline, the diagonal");
    assert_eq!(at(336, 245), canvas, "nothing right of the diagonal");
}

#[test]
fn scale_2x_output_picks_sharp_cursor_size() {
    let temp = TestTempDir::new("t1_cursor_scale");
    let icons_dir = temp.path().join("icons");

    // 24x24 red, 48x48 blue
    let red_pixels = [0x00, 0x00, 0xFF, 0xFF].repeat(24 * 24);
    let blue_pixels = [0xFF, 0x00, 0x00, 0xFF].repeat(48 * 48);
    write_cursor_theme(
        &icons_dir,
        "scale_theme",
        "default",
        &[
            (24, 24, 24, (0, 0), &red_pixels),
            (48, 48, 48, (0, 0), &blue_pixels),
        ],
    );

    write_config(temp.path(), "scale_theme", 24);

    // Output with scale_120 = 240 (2.0 scale)
    let mut ls = Lockstep::new(Some(output((640, 480), 240)));
    ls.state.set_console_target();
    ls.state.set_config_home(temp.path().to_path_buf());
    ls.state
        .set_xcursor_path(icons_dir.to_string_lossy().into_owned());
    ls.state.reload_config();

    // Logical view is 320x240, centre is (160, 120), move 5px right to (165, 120) -> physical (330, 240)
    nudge(&mut ls, (5.0, 0.0));
    let mut screen = Screen::new((640, 480));
    let (_, pixels) = screen.frame(&mut ls, 0);
    let at = |x: usize, y: usize| pixels[y * 640 + x];

    // At 2x scale, target size is 24 * 2 = 48, so the 48x48 blue image is chosen
    assert_eq!(at(330, 240), BLUE, "2x scale picked the 48px blue image");
}
