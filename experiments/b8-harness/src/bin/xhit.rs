//! CP3A hit-testing: "the surface selected by hit-testing is the surface presented at that location,
//! subject to Wayland input-region semantics" — at fractional output scales and fractional camera
//! positions, against Smithay's real render-element geometry.
//! Output: docs/foundation/evidence/phase-c/cp3a-xhit-output.txt
use b8_harness::{lockstep::Lockstep, server::*, snapshot};
use wayland_client::Proxy;
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

const DELTA: f64 = 1.0 / (1u64 << 20) as f64;

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

fn main() {
    println!("xhit — physical-pixel hit-testing vs presented surfaces; smithay =0.7.0 + DummyRenderer; lockstep");
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 60_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    let qh = ls.qh.clone();
    ls.server.pending_anchors.push_back((500, 400));
    let w = ls.probe.create_toplevel(&qh, "W", (400, 300), 20, false); // root 440x340, geometry (20,20)
    ls.pump();
    ls.run_until(10_000);
    // root input region: the window geometry MINUS a hole at (20,55) 20x30 (over the below-subsurface S2)
    ls.probe.set_input_ops(&qh, w, &[(true, (20, 20, 400, 300)), (false, (20, 55, 20, 30))]);
    let s1 = ls.probe.add_subsurface_ex(&ls.globals, &qh, w, "S1", (37, 11), (101, 53), false);
    let s1a = ls.probe.add_subsurface_ex(&ls.globals, &qh, s1, "S1a", (10, 10), (30, 20), false); // nested, above S1
    let s2 = ls.probe.add_subsurface_ex(&ls.globals, &qh, w, "S2", (-15, 50), (60, 40), true); // below root
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
    println!("tree front-to-back: {:?}", snap.surfaces.iter().map(|s| format!("{}@({},{}) {}x{} input={}", name(s.key), s.origin.x, s.origin.y, s.size.0, s.size.1, if s.input.is_some() { "region" } else { "all" })).collect::<Vec<_>>());

    let mut renderer = DummyRenderer::default();
    let root: WlSurface = window.toplevel().unwrap().wl_surface().clone();
    import_surface_tree(&mut renderer, &root).unwrap();
    for (pk, _) in PopupManager::popups_for_surface(&root) {
        import_surface_tree(&mut renderer, pk.wl_surface()).unwrap();
    }
    let anchor = (500i64, 400i64);
    let root_canvas = (anchor.0 - snap.geometry.loc.x as i64, anchor.1 - snap.geometry.loc.y as i64);

    // ---- semantic checks at k = 1, integral camera (pixel == logical), hand-derived expectations ----
    let hit_at = |snap: &snapshot::Snapshot, lx: i32, ly: i32| -> Option<String> {
        // RootSurfaceLocal point (pixel centre); front-to-back; half-open containment + region
        for s in &snap.surfaces {
            let local = (lx as f64 + 0.5 - s.origin.x as f64, ly as f64 + 0.5 - s.origin.y as f64);
            if local.0 >= 0.0 && local.1 >= 0.0 && local.0 < s.size.0 as f64 && local.1 < s.size.1 as f64 && eligible(s, local) {
                return Some(name(s.key));
            }
        }
        None
    };
    let expect: &[(&str, (i32, i32), Option<&str>)] = &[
        ("root window area", (200, 250), Some("W")),
        ("root CSD shadow (outside input region), nothing else there", (5, 5), None),
        ("S1 above root", (40, 15), Some("S1")),
        ("S1a nested above S1", (50, 25), Some("S1a")),
        ("S1 outside root's input region (S1 has no region)", (40, 12), Some("S1")),
        ("root over S2 (outside the hole): root wins (S2 is below)", (30, 88), Some("W")),
        ("input-region HOLE over S2: falls through to S2(below)", (25, 60), Some("S2(below)")),
        ("S2 left of the root (x<0)", (-10, 60), Some("S2(below)")),
        ("popup over root", (150, 100), Some("P")),
        ("popup subsurface PS", (120, 70), Some("PS")),
        // (an earlier expectation used (41,60), which lies inside S1 (y 11..64): the oracle was wrong, the hit-test right)
        ("right of the hole, below S1, inside the root input region", (41, 70), Some("W")),
        ("point inside S1 next to the hole (S1 above root)", (41, 60), Some("S1")),
    ];
    let mut sem_ok = true;
    println!("\n== semantic cases (k=1; RootSurfaceLocal pixel centres; hand-derived expectations)");
    for (what, (x, y), want) in expect {
        let got = hit_at(&snap, *x, *y);
        let ok = got.as_deref() == *want;
        sem_ok &= ok;
        println!("   [{}] {:<70} at ({:>3},{:>3}): got {:?} want {:?}", if ok { "PASS" } else { "FAIL" }, what, x, y, got, want);
    }

    // ---- fractional consistency: physical selection == presented surface ----
    println!("\n== physical-pixel selection vs presented surface (Smithay element geometry), all pixels over the tree");
    let mut all_ok = sem_ok;
    for &k in &[1.0f64, 1.25, 1.5, 1.75] {
        for &(vx, vy) in &[(400.0f64, 300.0f64), (400.3, 300.7), (401.71875, 299.55), (359.5, 241.25)] {
            let r = ((vx / 120.0).floor() * 120.0, (vy / 120.0).floor() * 120.0);
            let c = (((vx - r.0) * k + DELTA).round(), ((vy - r.1) * k + DELTA).round());
            let c_eff = (r.0 + c.0 / k, r.1 + c.1 / k);
            // per surface: Smithay presented rect (oracle) and core edge rect (implementation)
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
            let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
            for (_, g, _) in &rects {
                x0 = x0.min(g.loc.x - 3);
                y0 = y0.min(g.loc.y - 3);
                x1 = x1.max(g.loc.x + g.size.w + 3);
                y1 = y1.max(g.loc.y + g.size.h + 3);
            }
            let (mut n, mut mism, mut old_rule_mism, mut max_clamp) = (0u64, 0u64, 0u64, 0f64);
            for j in y0..y1 {
                for i in x0..x1 {
                    n += 1;
                    let q = (c_eff.0 + (i as f64 + 0.5) / k, c_eff.1 + (j as f64 + 0.5) / k); // Canvas point at pixel centre
                    let mut oracle = None;
                    let mut ours = None;
                    let mut old = None;
                    for (idx, g, e) in &rects {
                        let s = &snap.surfaces[*idx];
                        let origin = ((root_canvas.0 + s.origin.x as i64) as f64, (root_canvas.1 + s.origin.y as i64) as f64);
                        let local = (q.0 - origin.0, q.1 - origin.1);
                        if oracle.is_none() && g.contains((i, j)) && eligible(s, local) {
                            oracle = Some(*idx);
                        }
                        if ours.is_none() && i >= e.0 && i < e.2 && j >= e.1 && j < e.3 && eligible(s, local) {
                            ours = Some(*idx);
                            // delivered coordinate clamp distance (logical px)
                            let cx = (local.0.clamp(0.0, s.size.0 as f64) - local.0).abs();
                            let cy = (local.1.clamp(0.0, s.size.1 as f64) - local.1).abs();
                            max_clamp = max_clamp.max(cx.max(cy));
                        }
                        // Phase B D-B2-I9 rule: exact Canvas containment, no pixel coverage
                        if old.is_none() && local.0 >= 0.0 && local.1 >= 0.0 && local.0 < s.size.0 as f64 && local.1 < s.size.1 as f64 && eligible(s, local) {
                            old = Some(*idx);
                        }
                    }
                    if oracle != ours {
                        mism += 1;
                    }
                    if oracle != old {
                        old_rule_mism += 1;
                    }
                }
            }
            all_ok &= mism == 0;
            println!(
                "   k={:<4} camera v=({:>9.5},{:>9.5}) pixels={:>6} | physical-selection mismatches vs presented: {} | old canvas-exact rule mismatches: {:>4} | max coordinate clamp {:.3} logical px",
                k, vx, vy, n, mism, old_rule_mism, max_clamp
            );
        }
    }
    println!("\nALL: {}", if all_ok { "PASS" } else { "SOME FAILED" });
}
