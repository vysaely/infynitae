//! Seat physical pointer state (OBJ-20), core part: the authoritative `(OutputId, OutputLogical p)`
//! (D-B2-PTR-STATE, INV-COORD-5), moved by physical deltas in the Arrangement (B2.5 T1) and by the
//! output lifecycle (INV-OUTPUT-1).

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
/// (E-F2-05). `a = arr_origin(O_lost) + p`, then [`locate`].
pub(crate) fn relocate(
    lost: &OutputModel,
    p: (f64, f64),
    first: &OutputModel,
    rest: &[OutputModel],
) -> PointerState {
    locate(
        (lost.arrangement.0 + p.0, lost.arrangement.1 + p.1),
        first,
        rest,
    )
}

/// B2.5 T1 for a relative device: `a' = arr_origin(O) + p + Δ`, clamped to the union of the
/// output rects and located. `None` when `from` is not connected.
pub(crate) fn moved(
    outputs: &[OutputModel],
    from: OutputId,
    p: (f64, f64),
    delta: (f64, f64),
) -> Option<PointerState> {
    let o = outputs.iter().find(|o| o.id == from)?;
    let (first, rest) = outputs.split_first()?;
    Some(locate(
        (
            o.arrangement.0 + p.0 + delta.0,
            o.arrangement.1 + p.1 + delta.1,
        ),
        first,
        rest,
    ))
}

/// An Arrangement point located on the output whose rect is nearest to it (distance 0 when a rect
/// contains it; ties: first in order), clamped half-open into that rect: the nearest point of the
/// union of the output rects. The output set is non-empty by signature.
pub(crate) fn locate(a: (f64, f64), first: &OutputModel, rest: &[OutputModel]) -> PointerState {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{OutputDescriptor, Transform};

    fn model(id: u64, mode: (u32, u32), arrangement: (f64, f64)) -> OutputModel {
        let desc = OutputDescriptor {
            key: crate::output::tests::key("X"),
            mode,
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        };
        OutputModel::from_descriptor(OutputId::for_test(id), &desc, arrangement)
    }

    /// The nearest point of the union of output rects, half-open; ties go to the first output.
    #[test]
    fn locate_clamps_to_the_union_of_output_rects() {
        let a = model(1, (100, 100), (0.0, 0.0));
        let b = model(2, (50, 50), (100.0, 0.0));
        let at = |p: (f64, f64)| locate(p, &a, std::slice::from_ref(&b));
        let eps = WL_FIXED_EPS;
        assert_eq!(
            at((120.0, 10.0)),
            PointerState::Located {
                output: b.id,
                p: (20.0, 10.0)
            }
        );
        assert_eq!(
            at((100.0, 10.0)),
            PointerState::Located {
                output: b.id,
                p: (0.0, 10.0)
            }
        );
        assert_eq!(
            at((120.0, 70.0)),
            PointerState::Located {
                output: a.id,
                p: (100.0 - eps, 70.0)
            }
        );
        assert_eq!(
            at((-5.0, -5.0)),
            PointerState::Located {
                output: a.id,
                p: (0.0, 0.0)
            }
        );
        assert_eq!(
            at((1e9, 10.0)),
            PointerState::Located {
                output: b.id,
                p: (50.0 - eps, 10.0)
            }
        );
        // far below-right, A's corner (100, 100) is nearer than B's (150, 50)
        assert_eq!(
            at((1e9, 1e9)),
            PointerState::Located {
                output: a.id,
                p: (100.0 - eps, 100.0 - eps)
            }
        );
        // equidistant from both rects (A's half-open edge is 100 − 2⁻⁸): the first in order
        let c = model(3, (100, 100), (200.0, 0.0));
        assert_eq!(
            locate((149.998046875, 200.0), &a, std::slice::from_ref(&c)),
            PointerState::Located {
                output: a.id,
                p: (100.0 - eps, 100.0 - eps)
            }
        );
    }
}
