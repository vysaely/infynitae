//! INC-10 T1 (exit item 7, checkpoint E): surface and output lifecycle while the camera is zoomed
//! (k ∉ n/120) and while it pans, on the production path with a real client.
//!
//! After every step — a second window mapped, unmapped and destroyed, a popup opened and closed, a
//! subsurface added, during a zoomed pan; a fit; an output reconfiguration; a disconnect and a
//! reconnect in the middle of a pan — the output's frame is consistent: every presented rect is
//! Core's canonical rect, membership is exactly the surfaces in the render list (F-MEMBER-1),
//! every probed pixel hits the surface drawn there (INV-HIT-1), and no client action moved the
//! camera (INV-CAMERA-1). W's widths stay constant through the pan (the kept reference).

mod common;

use common::input::*;
use common::{Lockstep, PopupSpec, output};
use infynitae_backend_headless::{VirtualOutput, keyboard_key, render_frame};
use infynitae_core::geom::{CanvasPos, OutputLogicalPoint};
use infynitae_core::input::keys::{KEY_F, KEY_LEFTMETA};
use infynitae_core::navigation::{CameraValue, PersistedCameras};
use infynitae_core::output::OutputId;
use smithay::backend::renderer::element::Element;
use smithay::backend::renderer::test::DummyRenderer;
use smithay::utils::Scale;

fn camera(r: &Rig) -> Option<CameraValue> {
    let c = r.ls.state.core().cameras();
    Some(CameraValue {
        center: c.center(0)?,
        zoom: c.zoom(0)?,
    })
}

fn same(a: Option<CameraValue>, b: Option<CameraValue>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.center == b.center && a.zoom.to_bits() == b.zoom.to_bits(),
        _ => false,
    }
}

fn fit_key(r: &mut Rig) {
    let mut press = |key, pressed| {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    };
    press(KEY_LEFTMETA, true);
    press(KEY_F, true);
    press(KEY_F, false);
    press(KEY_LEFTMETA, false);
    r.frame();
}

/// The frame of `o` checked as the module doc says; returns W's surface sizes (root first).
fn consistent(r: &mut Rig, o: OutputId, what: &str) -> Vec<(i64, i64)> {
    let elements = render_frame(&mut r.ls.state, &mut DummyRenderer, o).expect("frame");
    let frame = r.ls.state.core_mut().render_list(o).expect("frame").clone();
    let k = frame.view.k;
    assert_eq!(elements.len(), frame.entries.len(), "{what}");
    for (e, el) in &elements {
        let g = el.geometry(Scale::from(k));
        let shown = (
            g.loc.x as i64,
            g.loc.y as i64,
            (g.loc.x + g.size.w) as i64,
            (g.loc.y + g.size.h) as i64,
        );
        assert_eq!(
            shown,
            (e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1),
            "{what}: presented ≠ Core"
        );
    }
    // F-MEMBER-1: membership on O is exactly the render list's surfaces
    let core = r.ls.state.core();
    let mut rows: Vec<_> = core
        .membership()
        .rows()
        .iter()
        .filter(|(_, _, out)| *out == o)
        .map(|(el, s, _)| (*el, *s))
        .collect();
    let mut listed: Vec<_> = frame
        .entries
        .iter()
        .map(|e| (e.element, e.surface))
        .collect();
    rows.sort_by_key(|(e, s)| (format!("{e:?}"), s.raw().get()));
    listed.sort_by_key(|(e, s)| (format!("{e:?}"), s.raw().get()));
    assert_eq!(rows, listed, "{what}: membership ≠ render list");
    // INV-HIT-1: a grid of pixels hits the surface drawn there (default input regions)
    let (vw, vh) = frame.view.size;
    for y in (3..vh as i64).step_by(41) {
        for x in (5..vw as i64).step_by(37) {
            let p = OutputLogicalPoint {
                x: (x as f64 + 0.5) / frame.view.scale,
                y: (y as f64 + 0.5) / frame.view.scale,
            };
            let hit = r.ls.state.core_mut().hit_test(o, p).map(|h| h.surface);
            let drawn = frame
                .entries
                .iter()
                .find(|e| x >= e.rect.x0 && x < e.rect.x1 && y >= e.rect.y0 && y < e.rect.y1)
                .map(|e| e.surface);
            assert_eq!(hit, drawn, "{what}: pixel ({x},{y}) hit ≠ drawn");
        }
    }
    let w = r.ls.state.elements().next().expect("W");
    frame
        .entries
        .iter()
        .filter(|e| e.element == w)
        .map(|e| (e.rect.x1 - e.rect.x0, e.rect.y1 - e.rect.y0))
        .collect()
}

#[test]
fn lifecycle_while_zoomed_and_panning_keeps_render_hit_and_membership_consistent() {
    let zoom = 0.73; // k ∉ n/120
    let persisted = PersistedCameras::from_entries(vec![(
        VirtualOutput::key("A"),
        CanvasPos { x: 0, y: 0 },
        zoom,
    )])
    .unwrap();
    let mut r = Rig::new(Lockstep::with(persisted, &[output((1920, 1080), 120)]));
    r.map_w();
    r.motion((40.0, 30.0));
    let mut o = r.ls.output();
    let widths = consistent(&mut r, o, "start");
    let qh = r.ls.qh.clone();

    // a zoomed pan, with client lifecycle in its middle
    r.button(BTN_MIDDLE, true, logo());
    // W's root (last in snapshot order) keeps its physical size through the pan; surfaces added
    // during it (a popup, a subsurface) keep theirs from when they appear
    let mut sizes = widths.clone();
    let mut step = |r: &mut Rig, d: (f64, f64), what: &str| {
        r.motion(d);
        let o = r.ls.output();
        let now = consistent(r, o, what);
        assert_eq!(
            now.last(),
            widths.last(),
            "{what}: W's root size changed during the pan"
        );
        if now.len() == sizes.len() {
            assert_eq!(
                now, sizes,
                "{what}: a surface's size changed during the pan"
            );
        }
        sizes = now;
    };
    step(&mut r, (11.3, -3.7), "pan 1");
    let before = camera(&r);
    let v = r.ls.probe.toplevel(&qh, "V", (300, 200), 10);
    r.ls.pump();
    r.frame();
    assert!(same(camera(&r), before), "a map moved the camera");
    consistent(&mut r, o, "V mapped");
    step(&mut r, (7.1, 9.9), "pan 2");
    let before = camera(&r);
    let p = r.ls.probe.popup(
        &qh,
        "P",
        0,
        PopupSpec {
            content: (120, 80),
            shadow: 8,
            anchor_rect: (100, 60, 10, 10),
            slide: false,
        },
    );
    r.ls.pump();
    r.frame();
    assert!(same(camera(&r), before), "a popup moved the camera");
    consistent(&mut r, o, "popup");
    let before = camera(&r);
    r.ls.probe
        .subsurface(&qh, "S", 0, (37, 41), (90, 70), false);
    r.ls.pump();
    r.ls.probe.surfaces[0].wl.commit();
    r.ls.pump();
    r.frame();
    assert!(same(camera(&r), before), "a subsurface moved the camera");
    consistent(&mut r, o, "subsurface");
    step(&mut r, (-5.5, 2.2), "pan 3");
    let before = camera(&r);
    r.ls.probe.unmap(v);
    r.ls.pump();
    r.frame();
    consistent(&mut r, o, "V unmapped");
    r.ls.probe.destroy_popup(p);
    r.ls.pump();
    r.frame();
    consistent(&mut r, o, "popup destroyed");
    r.ls.probe.destroy_toplevel(v);
    r.ls.pump();
    r.frame();
    assert!(same(camera(&r), before), "unmap/destroy moved the camera");
    consistent(&mut r, o, "V destroyed");
    step(&mut r, (3.0, 3.0), "pan 4");
    r.button(BTN_MIDDLE, false, logo());
    consistent(&mut r, o, "pan released");

    // a fit, then an output reconfiguration under the fitted camera
    fit_key(&mut r);
    assert_eq!(r.ls.state.core().cameras().history(0).unwrap().len(), 1);
    consistent(&mut r, o, "fitted");
    let fitted = camera(&r);
    r.ls.reconfigure(o, &common::output_k("A", (1920, 1080), 150, 60_000));
    r.frame();
    assert!(
        same(camera(&r), fitted),
        "a reconfiguration changed the camera"
    );
    assert_eq!(
        r.ls.state.core().cameras().history(0).unwrap().len(),
        1,
        "kept"
    );
    consistent(&mut r, o, "reconfigured");

    // a disconnect in the middle of a pan, then a reconnect
    r.button(BTN_MIDDLE, true, logo());
    r.motion((4.0, -2.0));
    let panned = camera(&r);
    r.ls.disconnect(o);
    r.motion((50.0, 50.0)); // orphaned: consumed
    assert!(same(camera(&r), panned), "an orphaned pan moved the camera");
    o =
        r.ls.connect(&common::output_k("A", (1920, 1080), 150, 60_000));
    r.button(BTN_MIDDLE, false, logo());
    r.frame();
    assert!(
        same(camera(&r), panned),
        "the reconnect changed the camera's value"
    );
    assert!(
        r.ls.state.core().cameras().history(0).unwrap().is_empty(),
        "history emptied"
    );
    consistent(&mut r, o, "reconnected");
}
