//! Phase D (D5, epoch 1): the four-epoch audit tree under the D5 cameras, against Smithay's real
//! render-element geometry. Extends xhit (EC-20) to an effective scale k that is NOT of the form n/120
//! (camera zoom from a viewport-fit, k_A = 1080/651) and to the exact D5 cameras/anchor.
//! Output: docs/foundation/evidence/phase-d/d5hit-output.txt
use b8_harness::{lockstep::Lockstep, server::*, snapshot};
use smithay::{
    backend::renderer::{
        element::{surface::WaylandSurfaceRenderElement, Element, Kind},
        test::DummyRenderer,
        utils::import_surface_tree,
    },
    desktop::PopupManager,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Physical, Point, Rectangle, Scale},
    wayland::compositor::with_states,
};
use wayland_client::Proxy;

const DELTA: f64 = 1.0 / (1u64 << 20) as f64;

fn q16(x: f64) -> f64 {
    (x * 65536.0).round_ties_even() / 65536.0
}

/// Input-region eligibility, half-open containment on the (clamped) surface-local logical point.
fn eligible(s: &snapshot::SnapSurface, local: (f64, f64)) -> bool {
    let lx = local.0.clamp(0.0, s.size.0 as f64 - 1e-9).floor() as i32;
    let ly = local.1.clamp(0.0, s.size.1 as f64 - 1e-9).floor() as i32;
    match &s.input {
        None => true,
        Some(rects) => {
            let mut r = false;
            for (add, rect) in rects {
                if rect.contains((lx, ly)) {
                    r = *add;
                }
            }
            r
        }
    }
}

/// One output view: (name, physical mode, k = zoom*scale, view origin v (Q16, from the camera centre))
struct View {
    name: &'static str,
    mode: (i32, i32),
    k: f64,
    v: (f64, f64),
}

fn view(name: &'static str, mode: (i32, i32), scale: f64, zoom: f64, centre: (f64, f64)) -> View {
    let k = zoom * scale;
    let c = (q16(centre.0), q16(centre.1));
    // INV-CAMERA-3 / R7: v_O = centre - mode/(2k), quantised to Q16
    let v = (q16(c.0 - mode.0 as f64 / (2.0 * k)), q16(c.1 - mode.1 as f64 / (2.0 * k)));
    View { name, mode, k, v }
}

fn main() {
    println!("d5hit — D5 epoch-1 tree vs Smithay presented geometry; smithay =0.7.0 + DummyRenderer; lockstep");
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 165_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    let qh = ls.qh.clone();
    ls.server.pending_anchors.push_back((1100, 420));
    let w = ls.probe.create_toplevel(&qh, "W", (800, 600), 20, false); // root 840x640, geometry (20,20) 800x600
    ls.pump();
    ls.run_until(10_000);
    // root input region: window geometry minus a hole over the below-subsurface S2
    ls.probe.set_input_ops(&qh, w, &[(true, (20, 20, 800, 600)), (false, (20, 55, 20, 30))]);
    let s1 = ls.probe.add_subsurface_ex(&ls.globals, &qh, w, "S1", (37, 11), (101, 53), false);
    let s1a = ls.probe.add_subsurface_ex(&ls.globals, &qh, s1, "S1a", (10, 10), (30, 20), false);
    let s2 = ls.probe.add_subsurface_ex(&ls.globals, &qh, w, "S2", (-15, 50), (60, 40), true);
    ls.pump();
    ls.run_until(20_000);
    let p = ls.probe.create_popup(&qh, "P", w, (120, 80), 10, (100, 50, 1, 1));
    ls.pump();
    ls.run_until(30_000);
    let _ps = ls.probe.add_subsurface_ex(&ls.globals, &qh, p, "PS", (5, 5), (30, 30), false);
    ls.pump();
    ls.run_until(40_000);

    let window = ls.server.elements[0].window.clone();
    let snap = snapshot::extract(&window);
    let reg = snapshot::registry(&window);
    let root_key = ls.probe.surfaces[w].wl.id().protocol_id();
    let pop_key = ls.probe.surfaces[p].wl.id().protocol_id();
    let name = |k: u32| -> String {
        if k == root_key { "W".into() } else if k == pop_key { "P".into() } else if k as usize == s1 { "S1".into() } else if k as usize == s1a { "S1a".into() } else if k as usize == s2 { "S2(below)".into() } else { "PS".into() }
    };
    let anchor = (1100i64, 420i64);
    let root_canvas = (anchor.0 - snap.geometry.loc.x as i64, anchor.1 - snap.geometry.loc.y as i64);
    println!("anchor (Canvas) = {:?}; window geometry (RootSurfaceLocal) = {:?}; root origin (Canvas) = {:?}", anchor, snap.geometry, root_canvas);
    println!("tree front-to-back (Canvas origins):");
    for s in &snap.surfaces {
        println!(
            "   {:<10} Canvas origin ({:>5},{:>5}) size {:>3}x{:<3} input={}",
            name(s.key),
            root_canvas.0 + s.origin.x as i64,
            root_canvas.1 + s.origin.y as i64,
            s.size.0,
            s.size.1,
            match &s.input { None => "whole surface".to_string(), Some(r) => format!("{:?}", r.iter().map(|(a, r)| (*a, r.loc.x, r.loc.y, r.size.w, r.size.h)).collect::<Vec<_>>()) }
        );
    }

    let mut renderer = DummyRenderer::default();
    let root: WlSurface = window.toplevel().unwrap().wl_surface().clone();
    import_surface_tree(&mut renderer, &root).unwrap();
    for (pk, _) in PopupManager::popups_for_surface(&root) {
        import_surface_tree(&mut renderer, pk.wl_surface()).unwrap();
    }

    let zoom_a = 1080.0f64 / 651.0; // viewport-fit of a 1157x651 element on A, kept after navigation (C5.1)
    let mut views = vec![
        view("A (D5 epoch 1: zoom 1080/651, scale 1.0)", (1920, 1080), 1.0, zoom_a, (1650.3, 700.7)),
        view("B (D5 epoch 1: zoom 1, scale 1.5)", (3840, 2160), 1.5, 1.0, (2200.45, 900.15)),
        view("B stress (zoom 1.25, scale 1.5, k=1.875)", (3840, 2160), 1.5, 1.25, (2200.45, 900.15)),
    ];
    // epoch-2 style continuous pan of A (fractional steps), sampled
    for i in [7u32, 23, 61, 97] {
        views.push(view("A pan sample", (1920, 1080), 1.0, zoom_a, (1650.3 + i as f64 * 3.71, 700.7 - i as f64 * 0.29)));
    }

    let mut all_ok = true;
    for vw in &views {
        let k = vw.k;
        let r = ((vw.v.0 / 120.0).floor() * 120.0, (vw.v.1 / 120.0).floor() * 120.0);
        let c = (((vw.v.0 - r.0) * k + DELTA).round(), ((vw.v.1 - r.1) * k + DELTA).round());
        let c_eff = (r.0 + q16(c.0 / k), r.1 + q16(c.1 / k));
        let mut rects: Vec<(usize, Rectangle<i32, Physical>, (i32, i32, i32, i32))> = Vec::new();
        for (i, s) in snap.surfaces.iter().enumerate() {
            let x = (root_canvas.0 + s.origin.x as i64) as f64;
            let y = (root_canvas.1 + s.origin.y as i64) as f64;
            let loc: Point<f64, Physical> = ((x - r.0) * k + DELTA - c.0, (y - r.1) * k + DELTA - c.1).into();
            let surf = &reg.iter().find(|(kk, _)| *kk == s.key).unwrap().1;
            let el = with_states(surf, |st| WaylandSurfaceRenderElement::<DummyRenderer>::from_surface(&mut renderer, surf, st, loc, 1.0, Kind::Unspecified)).ok().flatten().unwrap();
            let g = el.geometry(Scale::from(k));
            let edge = |v: f64, rr: f64, cc: f64| (((v - rr) * k + DELTA).round() - cc) as i32;
            let ours = (edge(x, r.0, c.0), edge(y, r.1, c.1), edge(x + s.size.0 as f64, r.0, c.0), edge(y + s.size.1 as f64, r.1, c.1));
            rects.push((i, g, ours));
        }
        println!("\n== {}: k={:.10} v=({:.6},{:.6}) R=({},{}) C=({},{}) c_eff=({:.6},{:.6})", vw.name, k, vw.v.0, vw.v.1, r.0, r.1, c.0, c.1, c_eff.0, c_eff.1);
        let mut edges_ok = true;
        for (i, g, e) in &rects {
            let s = &snap.surfaces[*i];
            let smithay = (g.loc.x, g.loc.y, g.loc.x + g.size.w, g.loc.y + g.size.h);
            let on = e.2 > 0 && e.0 < vw.mode.0 && e.3 > 0 && e.1 < vw.mode.1;
            edges_ok &= smithay == *e;
            println!("   {:<10} edges x[{},{}) y[{},{})  Smithay presented x[{},{}) y[{},{})  {}  on-output={}", name(s.key), e.0, e.2, e.1, e.3, smithay.0, smithay.2, smithay.1, smithay.3, if smithay == *e { "=" } else { "MISMATCH" }, on);
        }
        // every pixel over the tree: physical selection (core edges) vs presented (Smithay geometry)
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (_, g, _) in &rects {
            x0 = x0.min(g.loc.x - 3);
            y0 = y0.min(g.loc.y - 3);
            x1 = x1.max(g.loc.x + g.size.w + 3);
            y1 = y1.max(g.loc.y + g.size.h + 3);
        }
        let (mut n, mut mism, mut max_clamp) = (0u64, 0u64, 0f64);
        for j in y0..y1 {
            for i in x0..x1 {
                n += 1;
                let q = (c_eff.0 + q16((i as f64 + 0.5) / k), c_eff.1 + q16((j as f64 + 0.5) / k));
                let (mut oracle, mut ours) = (None, None);
                for (idx, g, e) in &rects {
                    let s = &snap.surfaces[*idx];
                    let origin = ((root_canvas.0 + s.origin.x as i64) as f64, (root_canvas.1 + s.origin.y as i64) as f64);
                    let local = (q.0 - origin.0, q.1 - origin.1);
                    if oracle.is_none() && g.contains((i, j)) && eligible(s, local) {
                        oracle = Some(*idx);
                    }
                    if ours.is_none() && i >= e.0 && i < e.2 && j >= e.1 && j < e.3 && eligible(s, local) {
                        ours = Some(*idx);
                        let cx = (local.0.clamp(0.0, s.size.0 as f64) - local.0).abs();
                        let cy = (local.1.clamp(0.0, s.size.1 as f64) - local.1).abs();
                        max_clamp = max_clamp.max(cx.max(cy));
                    }
                }
                if oracle != ours {
                    mism += 1;
                }
            }
        }
        all_ok &= edges_ok && mism == 0;
        println!("   pixels={} physical-selection mismatches vs presented={} edge table {} max delivered-coordinate clamp at pixel centres={:.3e} logical px", n, mism, if edges_ok { "equal" } else { "DIFFERS" }, max_clamp);
    }
    println!("\nALL: {}", if all_ok { "PASS" } else { "SOME FAILED" });
}
