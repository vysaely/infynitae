//! INV-PROTO-1 as amended by C12.F3, on the production path: no bounds are proposed.
//! `xdg_toplevel.configure_bounds` reaches a client only as `(0, 0)` and only together with an
//! initial configure, where Smithay emits it by itself (W-SM-17); xdg-shell defines zero
//! dimensions as "bounds unknown", equivalent to the event never having been sent. The oracle is
//! the client's own record of the events it received.

mod common;

use std::time::Duration;

use common::{Lockstep, output, output_k};
use infynitae_backend_headless::set_focus;

fn adv(ls: &mut Lockstep) {
    ls.pump();
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
}

/// Every `configure_bounds` the toplevel `idx` has received, in order.
fn bounds_of(ls: &Lockstep, idx: usize) -> Vec<(i32, i32)> {
    ls.probe
        .configure_bounds
        .iter()
        .filter(|(i, _)| *i == idx)
        .map(|(_, b)| *b)
        .collect()
}

/// Through everything that could plausibly change a bound — focus, a maximize request, an output
/// reconfiguration to another size and scale, a second output, an unmap and a re-show — the only
/// `configure_bounds` a client sees is `(0, 0)`, once per initial configure and never between.
#[test]
fn configure_bounds_is_zero_and_only_with_an_initial_configure() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (400, 300), 0);
    adv(&mut ls);
    assert_eq!(
        bounds_of(&ls, w),
        vec![(0, 0)],
        "with the initial configure"
    );
    let id = ls.state.elements().next().expect("element");

    set_focus(&mut ls.state, &mut ls.input, Some(id)).expect("focus");
    adv(&mut ls);
    ls.probe.surfaces[w]
        .toplevel
        .as_ref()
        .expect("toplevel")
        .set_maximized();
    adv(&mut ls);
    let o = ls.output();
    ls.reconfigure(o, &output_k("A", (2560, 1440), 180, 60_000));
    adv(&mut ls);
    ls.connect(&output_k("B", (1280, 720), 120, 60_000));
    adv(&mut ls);
    assert!(
        ls.probe.toplevel_configures.len() >= 3,
        "the window was configured several times: {:?}",
        ls.probe.toplevel_configures
    );
    assert_eq!(
        bounds_of(&ls, w),
        vec![(0, 0)],
        "a later configure carried bounds"
    );

    // unmap and re-show: a new initial configure, and with it at most one more (0, 0)
    ls.probe.unmap(w);
    adv(&mut ls);
    assert_eq!(bounds_of(&ls, w), vec![(0, 0)], "an unmap carried bounds");
    ls.probe.surfaces[w].wl.commit();
    adv(&mut ls);
    let after = bounds_of(&ls, w);
    assert!(
        after.len() <= 2,
        "more bounds than initial configures: {after:?}"
    );
    assert!(
        after.iter().all(|b| *b == (0, 0)),
        "a non-zero bound was proposed: {after:?}"
    );

    // a second toplevel gets its own, and the first gets nothing more
    let x = ls.probe.toplevel(&qh, "X", (300, 200), 0);
    adv(&mut ls);
    assert_eq!(bounds_of(&ls, x), vec![(0, 0)]);
    assert_eq!(bounds_of(&ls, w), after);
}
