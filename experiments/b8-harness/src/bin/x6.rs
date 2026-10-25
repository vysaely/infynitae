//! X-6: Smithay's real render-element edges vs the B1.5 lattice rule, for a root surface and an
//! odd-offset subsurface, across all fractional scales n/120 (n = 120..=360) and camera magnitudes.
//! Output: docs/archive/evidence/phase-b/x6-edges-output.txt

use b8_harness::{lockstep::Lockstep, server::*};
use smithay::{
    backend::renderer::{
        element::{surface::WaylandSurfaceRenderElement, Element, Kind},
        test::DummyRenderer,
        utils::{import_surface_tree, with_renderer_surface_state},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Physical, Point, Scale},
    wayland::compositor::{get_children, with_states},
};

const DELTA: f64 = 1.0 / (1u64 << 20) as f64;

fn main() {
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 60_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    ls.server.pending_anchors.push_back((0, 0));
    let qh = ls.qh.clone();
    let w = ls.probe.create_toplevel(&qh, "W", (803, 601), 17, false); // odd sizes and shadow
    ls.pump();
    ls.run_until(10_000);
    ls.probe.add_subsurface(&ls.globals, &qh, w, "S", (37, 11), (101, 53));
    ls.pump();
    ls.run_until(20_000);

    let window = ls.server.elements[0].window.clone();
    let root: WlSurface = window.toplevel().unwrap().wl_surface().clone();
    let mut renderer = DummyRenderer::default();
    import_surface_tree(&mut renderer, &root).expect("import surface tree");
    let child = get_children(&root).into_iter().next().expect("subsurface");
    let off = with_renderer_surface_state(&child, |s| s.view().map(|v| v.offset)).flatten().expect("child view");
    let root_size = with_renderer_surface_state(&root, |s| s.view().map(|v| v.dst)).flatten().unwrap();
    let child_size = with_renderer_surface_state(&child, |s| s.view().map(|v| v.dst)).flatten().unwrap();
    println!("root surface {}x{}, subsurface offset from public renderer state = ({},{}) size {}x{}", root_size.w, root_size.h, off.x, off.y, child_size.w, child_size.h);

    let mut checked = 0u64;
    let mut bad_f64 = 0u64;
    let mut bad_i32 = 0u64;
    let mut worst_i32 = (0u64, 0.0);
    for n in 120..=360i64 {
        let k = n as f64 / 120.0;
        let mut bad_i32_n = 0u64;
        for &base in &[0i64, 1_000_003, (1i64 << 36) - 77_777] {
            for dx in (0..1200).step_by(13) {
                // element root surface at Canvas x_root, camera view origin v (integral here)
                let x_root = base + dx;
                let y_root = base / 3 + dx / 2;
                let v = (base - 400 + dx % 7, base / 3 - 300);
                let r = (v.0.div_euclid(120) * 120, v.1.div_euclid(120) * 120);
                let c = (((v.0 - r.0) as f64 * k + DELTA).round(), ((v.1 - r.1) as f64 * k + DELTA).round());
                let edge = |x: i64, rr: i64, cc: f64| ((x - rr) as f64 * k + DELTA).round() - cc;
                for (surf, ox, oy, w, h) in [(&root, 0i64, 0i64, root_size.w as i64, root_size.h as i64), (&child, off.x as i64, off.y as i64, child_size.w as i64, child_size.h as i64)] {
                    let (x, y) = (x_root + ox, y_root + oy);
                    // f64 location exactly as decided in B1.5
                    let loc: Point<f64, Physical> = (((x - r.0) as f64 * k + DELTA - c.0), ((y - r.1) as f64 * k + DELTA - c.1)).into();
                    let el = with_states(surf, |st| WaylandSurfaceRenderElement::<DummyRenderer>::from_surface(&mut renderer, surf, st, loc, 1.0, Kind::Unspecified))
                        .ok()
                        .flatten()
                        .expect("render element");
                    let g = el.geometry(Scale::from(k));
                    let want = (edge(x, r.0, c.0) as i32, edge(y, r.1, c.1) as i32, edge(x + w, r.0, c.0) as i32, edge(y + h, r.1, c.1) as i32);
                    let got = (g.loc.x, g.loc.y, g.loc.x + g.size.w, g.loc.y + g.size.h);
                    checked += 1;
                    if got != want {
                        bad_f64 += 1;
                    }
                    // the i32-root alternative: round the root location first, offsets added by Smithay
                    let root_i32: Point<i32, Physical> = (((x_root - r.0) as f64 * k - c.0).round() as i32, ((y_root - r.1) as f64 * k - c.1).round() as i32).into();
                    let loc_i = root_i32.to_f64() + Point::<f64, Physical>::from((ox as f64 * k, oy as f64 * k));
                    let el_i = with_states(surf, |st| WaylandSurfaceRenderElement::<DummyRenderer>::from_surface(&mut renderer, surf, st, loc_i, 1.0, Kind::Unspecified)).ok().flatten().unwrap();
                    let gi = el_i.geometry(Scale::from(k));
                    if (gi.loc.x, gi.loc.y, gi.loc.x + gi.size.w, gi.loc.y + gi.size.h) != want {
                        bad_i32 += 1;
                        bad_i32_n += 1;
                    }
                }
            }
        }
        if bad_i32_n > worst_i32.0 {
            worst_i32 = (bad_i32_n, k);
        }
    }
    println!("surface rects checked (root + subsurface, 241 scales x 3 magnitudes x 93 positions): {checked}");
    println!("f64 root location (B1.5 rule): rects whose 4 edges differ from edge(x): {bad_f64}");
    println!("i32 root location (render_elements_from_surface_tree style): rects differing: {bad_i32} (worst scale {:.4}: {})", worst_i32.1, worst_i32.0);
}
