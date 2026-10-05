//! Seat physical pointer state (OBJ-20), core part: transitions caused by output lifecycle only
//! (INV-OUTPUT-1). Pointer intake, routing and the adapter's zero-output grab arrive with INC-3.

use crate::output::{OutputId, OutputModel};

/// wl_fixed resolution (2⁻⁸): the half-open clamp bound of F-RELOCATE-1 and F-RECONF-1.
pub const WL_FIXED_EPS: f64 = 1.0 / 256.0;

/// `Located { output, p }` with p in the output's half-open OutputLogical extent, or `NoOutput`
/// (no output exists: nothing is fabricated).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerState {
    Located { output: OutputId, p: (f64, f64) },
    NoOutput,
}

/// Clamp into `[0, extent − 2⁻⁸]` per coordinate.
fn clamp_halfopen(p: (f64, f64), extent: (f64, f64)) -> (f64, f64) {
    (
        p.0.clamp(0.0, extent.0 - WL_FIXED_EPS),
        p.1.clamp(0.0, extent.1 - WL_FIXED_EPS),
    )
}

/// Startup rule: the centre of the new output's logical extent (INV-OUTPUT-1).
pub(crate) fn startup(output: &OutputModel) -> PointerState {
    let e = output.logical_extent();
    PointerState::Located {
        output: output.id,
        p: clamp_halfopen((e.0 / 2.0, e.1 / 2.0), e),
    }
}

/// F-RECONF-1: p clamped into the reconfigured output's new logical extent; a p already inside
/// is unchanged. No camera value is read.
pub(crate) fn reconfigured(state: PointerState, output: &OutputModel) -> PointerState {
    match state {
        PointerState::Located { output: o, p } if o == output.id => PointerState::Located {
            output: o,
            p: clamp_halfopen(p, output.logical_extent()),
        },
        other => other,
    }
}

/// F-RELOCATE-1 on loss of the interaction output. Its domain is a NON-EMPTY survivor set, so the
/// signature takes the first survivor separately: evaluating it on an empty set is unrepresentable
/// (E-F2-05). `a = arr_origin(O_lost) + p`; the survivor whose Arrangement rect is nearest to `a`
/// (ties: first in order); `a` clamped half-open into it; `p' = a − arr_origin(O')`.
pub(crate) fn relocate(
    lost: &OutputModel,
    p: (f64, f64),
    first: &OutputModel,
    rest: &[OutputModel],
) -> PointerState {
    let a = (lost.arrangement.0 + p.0, lost.arrangement.1 + p.1);
    let candidate = |o: &OutputModel| {
        let local = clamp_halfopen(
            (a.0 - o.arrangement.0, a.1 - o.arrangement.1),
            o.logical_extent(),
        );
        let d =
            (o.arrangement.0 + local.0 - a.0).powi(2) + (o.arrangement.1 + local.1 - a.1).powi(2);
        (d, o.id, local)
    };
    let mut best = candidate(first);
    for o in rest {
        let c = candidate(o);
        if c.0 < best.0 {
            best = c;
        }
    }
    PointerState::Located {
        output: best.1,
        p: best.2,
    }
}
