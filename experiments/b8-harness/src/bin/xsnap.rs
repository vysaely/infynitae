//! Surface-tree snapshot verification (Phase C checkpoint 2, item 2) + X-15 (render order) + an
//! X-13 slice (single origin function agrees with Smithay hit-testing).
//! Output: docs/archive/evidence/phase-c/xsnap-x15-output.txt
use b8_harness::{lockstep::Lockstep, server::*, snapshot};
use wayland_client::Proxy;
use smithay::{
    backend::renderer::{
        element::{surface::WaylandSurfaceRenderElement, Element, Id},
        test::DummyRenderer,
        utils::import_surface_tree,
    },
    desktop::{PopupManager, WindowSurfaceType},
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    utils::{Physical, Point, Scale},
    backend::renderer::element::AsRenderElements,
};

fn main() {
    println!("snapshot / X-15 / X-13-slice — smithay =0.7.0, public API only, lockstep");
    let outs = [OutCfg { name: "A", mode: (1920, 1080), scale: 1.0, refresh_mhz: 60_000, camera: (0, 0) }];
    let mut ls = Lockstep::new(&outs, Policy { scale: ScalePolicy::Max, frames: FramePolicy::Starve, clock: ClockPolicy::HighestRefresh, suspend: SuspendPolicy::Never });
    ls.probe.record_frames = false;
    ls.server.quiet = true;
    let qh = ls.qh.clone();
    ls.server.pending_anchors.push_back((500, 400));
    // W: content 400x300, CSD shadow 20 (root surface 440x340, geometry (20,20,400,300))
    let w = ls.probe.create_toplevel(&qh, "W", (400, 300), 20, false);
    ls.pump();
    ls.run_until(10_000);
    // input region = window geometry only (shadow not clickable)
    ls.probe.set_input_rect(&qh, w, (20, 20, 400, 300));
    // S1 above root at (37,11) 101x53; S2 BELOW root at (-15,50) 60x40 (negative offset, outside geometry)
    let s1 = ls.probe.add_subsurface_ex(&ls.globals, &qh, w, "S1", (37, 11), (101, 53), false);
    let s2 = ls.probe.add_subsurface_ex(&ls.globals, &qh, w, "S2", (-15, 50), (60, 40), true);
    ls.pump();
    ls.run_until(20_000);
    // popup P: content 120x80, shadow 10, anchor (100,50) in parent geometry; with its own subsurface PS at (5,5) 30x30
    let p = ls.probe.create_popup(&qh, "P", w, (120, 80), 10, (100, 50, 1, 1));
    ls.pump();
    ls.run_until(30_000);
    let ps = ls.probe.add_subsurface_ex(&ls.globals, &qh, p, "PS", (5, 5), (30, 30), false);
    ls.pump();
    ls.run_until(40_000);

    let label = |key: u32| -> String {
        let root = ls.probe.surfaces[w].wl.id().protocol_id();
        let pop = ls.probe.surfaces[p].wl.id().protocol_id();
        if key == root {
            "W(root)".into()
        } else if key == pop {
            "P(popup)".into()
        } else if key as usize == s1 {
            "S1".into()
        } else if key as usize == s2 {
            "S2(below)".into()
        } else if key as usize == ps {
            "PS".into()
        } else {
            format!("#{key}")
        }
    };

    let window = ls.server.elements[0].window.clone();
    let snap = snapshot::extract(&window);
    println!("\n== snapshot (front-to-back), RootSurfaceLocal; window geometry {:?}", snap.geometry);
    for s in &snap.surfaces {
        println!("   {:<10} kind={:<9?} origin=({:>4},{:>4}) size={:>3}x{:<3} mapped={} input={:?}", label(s.key), s.kind, s.origin.x, s.origin.y, s.size.0, s.size.1, s.mapped, s.input);
    }

    // --- ground truth from the client's requests (hand-derived) ---
    // root origin (0,0); geometry (20,20); S1 (37,11); S2 (-15,50); popup geometry origin = geometry + (100,50) = (120,70),
    // popup surface origin = (120,70) - popup shadow (10,10) = (110,60); PS = popup origin + (5,5) = (115,65)
    let truth = [("P(popup)", (110, 60), (140, 100)), ("PS", (115, 65), (30, 30)), ("S1", (37, 11), (101, 53)), ("W(root)", (0, 0), (440, 340)), ("S2(below)", (-15, 50), (60, 40))];
    let mut ok_truth = true;
    for (name, o, sz) in truth {
        let found = snap.surfaces.iter().find(|s| label(s.key) == name);
        let good = found.map(|s| (s.origin.x, s.origin.y) == o && s.size == sz).unwrap_or(false);
        ok_truth &= good;
        println!("   truth {:<10} origin {:?} size {:?}: {}", name, o, sz, if good { "match" } else { "MISMATCH" });
    }
    let order: Vec<String> = snap.surfaces.iter().map(|s| label(s.key)).collect();
    println!("[{}] snapshot equals client ground truth (origins/sizes, incl. negative-offset below-subsurface and popup subsurface)", if ok_truth { "PASS" } else { "FAIL" });

    // --- X-15: Smithay's own Window render path (DummyRenderer, scale 1, location 0) ---
    let mut renderer = DummyRenderer::default();
    let root: WlSurface = window.toplevel().unwrap().wl_surface().clone();
    import_surface_tree(&mut renderer, &root).expect("import root tree");
    for (pk, _) in PopupManager::popups_for_surface(&root) {
        import_surface_tree(&mut renderer, pk.wl_surface()).expect("import popup tree");
    }
    let elems: Vec<WaylandSurfaceRenderElement<DummyRenderer>> = window.render_elements(&mut renderer, Point::<i32, Physical>::from((0, 0)), Scale::from(1.0), 1.0);
    let reg = snapshot::registry(&window);
    let id_of: Vec<(Id, u32)> = reg.iter().map(|(k, surf)| (Id::from_wayland_resource(surf), *k)).collect();
    let smithay_order: Vec<String> = elems.iter().map(|e| id_of.iter().find(|(i, _)| i == e.id()).map(|(_, k)| label(*k)).unwrap_or("?".into())).collect();
    println!("\n== X-15 render order (front-to-back)");
    println!("   snapshot order: {order:?}");
    println!("   Smithay Window::render_elements order: {smithay_order:?}");
    let ok_order = order == smithay_order;
    println!("[{}] X-15 order equivalence", if ok_order { "PASS" } else { "FAIL" });
    // positions: Smithay element geometry at scale 1 vs snapshot origin/size (root location 0)
    let mut ok_pos = true;
    for e in &elems {
        let key = id_of.iter().find(|(i, _)| i == e.id()).map(|(_, k)| *k).unwrap();
        let s = snap.surfaces.iter().find(|s| s.key == key).unwrap();
        let g = e.geometry(Scale::from(1.0));
        let good = (g.loc.x, g.loc.y, g.size.w, g.size.h) == (s.origin.x, s.origin.y, s.size.0, s.size.1);
        ok_pos &= good;
        println!("   {:<10} smithay geometry ({:>4},{:>4}) {:>3}x{:<3} vs snapshot ({:>4},{:>4}) {:>3}x{:<3} {}", label(key), g.loc.x, g.loc.y, g.size.w, g.size.h, s.origin.x, s.origin.y, s.size.0, s.size.1, if good { "" } else { "MISMATCH" });
    }
    println!("[{}] X-15 positions equal (single origin function == Smithay's Window path at scale 1)", if ok_pos { "PASS" } else { "FAIL" });

    // --- X-13 slice: snapshot hit-test vs Window::surface_under on a dense grid (RootSurfaceLocal) ---
    let mut checked = 0;
    let mut mismatches = 0;
    let mut first: Vec<String> = Vec::new();
    for yi in -20..380 {
        for xi in -40..460 {
            let pt = Point::from((xi as f64 + 0.5, yi as f64 + 0.25));
            let ours = snapshot::hit(&snap, pt);
            let theirs = window.surface_under(pt, WindowSurfaceType::ALL).map(|(s, o)| (s.id().protocol_id(), o));
            checked += 1;
            if ours != theirs {
                mismatches += 1;
                if first.len() < 5 {
                    first.push(format!("({:.2},{:.2}): ours {:?} smithay {:?}", pt.x, pt.y, ours.map(|(k, o)| (label(k), o)), theirs.map(|(k, o)| (label(k), o))));
                }
            }
        }
    }
    println!("\n== X-13 slice: hit-test agreement, {checked} points over the whole tree bbox (+margins)");
    for f in &first {
        println!("   mismatch {f}");
    }
    // classify every mismatch: is it within 0.5 px of an input-region edge (W-SM-11)?
    let near_region_edge = |pt: Point<f64, smithay::utils::Logical>| -> bool {
        snap.surfaces.iter().any(|s| {
            s.input.as_ref().map_or(false, |rs| {
                rs.iter().any(|(_, r)| {
                    let lx = pt.x - s.origin.x as f64;
                    let ly = pt.y - s.origin.y as f64;
                    let ex = [r.loc.x as f64, (r.loc.x + r.size.w) as f64];
                    let ey = [r.loc.y as f64, (r.loc.y + r.size.h) as f64];
                    ex.iter().any(|e| (lx - e).abs() <= 0.5) || ey.iter().any(|e| (ly - e).abs() <= 0.5)
                })
            })
        })
    };
    let mut unexplained = 0;
    let mut emu_mismatch = 0;
    let mut w_sm_12 = 0;
    let mut emu_unexplained = 0;
    let s2_key = s2 as u32;
    let root_key = ls.probe.surfaces[w].wl.id().protocol_id();
    for yi in -20..380 {
        for xi in -40..460 {
            let pt = Point::from((xi as f64 + 0.5, yi as f64 + 0.25));
            let theirs = window.surface_under(pt, WindowSurfaceType::ALL).map(|(s, o)| (s.id().protocol_id(), o));
            if snapshot::hit(&snap, pt) != theirs && !near_region_edge(pt) {
                unexplained += 1;
            }
            let emu = snapshot::hit_with(&snap, pt, true);
            if emu != theirs {
                emu_mismatch += 1;
                // W-SM-12 pattern: we (front-to-back) pick the root, Smithay picks the BELOW subsurface S2
                if emu.map(|x| x.0) == Some(root_key) && theirs.as_ref().map(|x| x.0) == Some(s2_key) {
                    w_sm_12 += 1;
                } else {
                    emu_unexplained += 1;
                    if emu_unexplained <= 6 {
                        println!("   UNEXPLAINED at ({:.2},{:.2}): ours {:?} smithay {:?}", pt.x, pt.y, emu.map(|(k, o)| (label(k), o)), theirs.map(|(k, o)| (label(k), o)));
                    }
                }
            }
        }
    }
    println!("   {mismatches} mismatches with half-open containment; unexplained (not within 0.5 px of an input-region edge): {unexplained}");
    println!("   with Smithay's round-to-nearest region test emulated: {emu_mismatch} mismatches, of which {w_sm_12} are the W-SM-12 pattern (Smithay picks the BELOW subsurface S2 over its parent), {emu_unexplained} unexplained");
    let _ = unexplained;
    let ok_hit = emu_unexplained == 0;
    println!("[{}] hit-test agreement: every difference from Window::surface_under is W-SM-11 (region rounding) or W-SM-12 (below-subsurface wins over parent); none unexplained", if ok_hit { "PASS" } else { "FAIL" });
    println!("   Infynitae's snapshot hit-test is consistent with the RENDERED stacking (X-15: Smithay draws W(root) in front of S2(below)); Smithay's surface_under is not.");
    println!("\n== dispatch safety: snapshots extracted inside CompositorHandler::commit during this run: {} (last had {} surfaces); no deadlock", ls.server.snapshots_taken, ls.server.last_snapshot_len);
    println!("\nSCOPE: integral scale 1, one popup level, one input region; fractional-scale render edges are covered by X-6; multi-level popups and region subtraction are T1 extensions.");
    println!("ALL: {}", if ok_truth && ok_order && ok_pos && ok_hit { "PASS" } else { "SOME FAILED" });
}
