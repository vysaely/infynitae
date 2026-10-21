//! OB-53, INC-10 part ("the viewport-fit feedback loop of EC-22 (INV-CAMERA-5)"): an adversarial
//! T1 client under a viewport fit. Pass criterion of OB-53: "hint changes ≤ (rungs crossed + 1 +
//! one P7 per non-client stimulus) per epoch; the run ends quiescent; no camera mutation from
//! client geometry (INV-CAMERA-1); re-fit rate ≤ commit rate (EC-22)".
//!
//! Setup: one 1920 × 1080 output at scale 1, device bound D_max = 2000 px; W (800 × 600, with a
//! fractional-scale object and a viewport) under the pointer. Logo+F fits W: zoom 1.8, so W's
//! requirement ρ rises to 1.8 and its hint follows once (an atomic camera action is a one-step
//! epoch, INV-SCALE-3). The adversary then answers every hint: at a hint ≥ 1.66 it grows to
//! 1300 px wide (1300 · h > D_max: P7 must lower the hint at once), at a lower hint it shrinks
//! back to 800 px (which would allow the higher hint again: a client-caused reversal, forbidden by
//! SC-E). Each answer is a commit with new window geometry; none may move the camera.
//!
//! Expected (hand-derived from F-SCALE-2/3 and INV-SCALE-3/4): after the fit's own change, at most
//! one further change (the P7 decrease), every 1300-px buffer within D_max at the hint in force,
//! the last rounds without any hint change, the camera exactly the fitted value and one history
//! entry throughout (no re-fit: the fit is one-shot).

mod common;

use common::Lockstep;
use common::input::*;
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::{KEY_ENTER, KEY_LEFTMETA};
use infynitae_core::navigation::CameraValue;

const D_MAX: u32 = 2000;

fn logo_f(r: &mut Rig) {
    for (key, pressed) in [
        (KEY_LEFTMETA, true),
        (KEY_ENTER, true),
        (KEY_ENTER, false),
        (KEY_LEFTMETA, false),
    ] {
        r.t += 1000;
        keyboard_key(&mut r.ls.state, &mut r.ls.input, key, pressed, r.t).expect("key");
        r.ls.pump();
    }
    r.frame();
}

fn camera(r: &Rig) -> CameraValue {
    let c = r.ls.state.core().cameras();
    CameraValue {
        center: c.center(0).unwrap(),
        zoom: c.zoom(0).unwrap(),
    }
}

fn history_len(r: &Rig) -> usize {
    r.ls.state
        .core()
        .cameras()
        .history(0)
        .map_or(0, |h| h.len())
}

#[test]
fn an_adversarial_client_under_a_fit_changes_the_hint_at_most_once_more_and_never_moves_the_camera()
{
    let mut ls = Lockstep::new(Some(common::output_k("A", (1920, 1080), 120, 60_000)));
    let fx = ls
        .state
        .core_mut()
        .set_renderer_device_max(D_MAX)
        .expect("valid D_max");
    ls.state.apply(&fx);
    let w = ls
        .probe
        .toplevel_with_fractional_scale(&ls.qh, "W", (800, 600), 0);
    ls.pump();
    ls.probe.draw(&ls.qh, w);
    ls.pump();
    let vp = ls.probe.get_viewport(&ls.qh, w).expect("viewport");
    vp.set_destination(800, 600);
    ls.probe.surfaces[w].wl.commit();
    ls.pump();
    let mut r = Rig::new(ls);
    r.frame();
    let before_fit = r.ls.probe.preferred_scales_of("W").len();
    let h0 = *r
        .ls
        .probe
        .preferred_scales_of("W")
        .last()
        .expect("an initial hint");

    logo_f(&mut r);
    assert_eq!(history_len(&r), 1, "the fit happened (W under the pointer)");
    let fitted = camera(&r);
    assert!((fitted.zoom - 1.8).abs() < 1e-12, "zoom {}", fitted.zoom);
    let after_fit: Vec<u32> = r.ls.probe.preferred_scales_of("W")[before_fit..].to_vec();
    assert_eq!(
        after_fit.len(),
        1,
        "the fit is one step: one change ({h0} → {after_fit:?})"
    );
    assert!(after_fit[0] > h0, "ρ rose to 1.8: the hint rises");

    let start = r.ls.probe.preferred_scales_of("W").len();
    let mut per_round = Vec::new();
    let mut sides = Vec::new();
    for round in 0..30 {
        let hint = *r.ls.probe.preferred_scales_of("W").last().unwrap();
        // the adversary: wide while its hint is high, narrow while it is low
        let side = if u64::from(hint) * 1200 > u64::from(D_MAX) * 120 {
            1300
        } else {
            800
        };
        sides.push(side);
        let n = r.ls.probe.preferred_scales_of("W").len();
        vp.set_destination(side, 600);
        r.ls.probe.surfaces[w].wl.commit();
        r.ls.pump();
        r.frame();
        let now = *r.ls.probe.preferred_scales_of("W").last().unwrap();
        per_round.push(r.ls.probe.preferred_scales_of("W").len() - n);
        assert!(
            side == 800 || u64::from(now) * 1300 <= u64::from(D_MAX) * 120,
            "round {round}: a {side}-px buffer at hint {now}/120 exceeds D_max (P7)"
        );
        assert_eq!(
            camera(&r),
            fitted,
            "round {round}: client geometry moved the camera (INV-CAMERA-1, INV-CAMERA-5)"
        );
        assert_eq!(history_len(&r), 1, "round {round}: something re-fitted");
    }
    let changes: Vec<u32> = r.ls.probe.preferred_scales_of("W")[start..].to_vec();
    // the scenario happened: the fitted hint was too high for a 1300-px buffer, so the adversary
    // grew at once and P7 lowered the hint; afterwards it shrank and SC-E kept the hint down
    assert_eq!(sides[0], 1300, "the adversary must grow first: {sides:?}");
    assert!(sides.contains(&800), "and shrink back: {sides:?}");
    assert_eq!(
        changes.len(),
        1,
        "after the fit: exactly the one P7 decrease, got {changes:?}"
    );
    assert!(
        changes[0] < after_fit[0],
        "P7 decreases: {after_fit:?} → {changes:?}"
    );
    assert!(
        per_round[10..].iter().all(|&c| c == 0),
        "the run must end quiescent: {per_round:?}"
    );
}
