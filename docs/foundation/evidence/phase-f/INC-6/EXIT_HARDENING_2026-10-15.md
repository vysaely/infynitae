# INC-6 exit hardening on the INC-7 integration — record (2026-10-15)

Status: evidence, not authority. Branch `integration/inc-6-exit-hardening-2026-10-15`, from the
rehearsal `rehearsal/inc-6-on-main-23752f1` @ `2753b35` (INC-6 `f1ef262` merged onto ratified main
`23752f1`). **INC-6 is not ratified; no obligation is closed by this record.** The findings of the
INC-6 hardening report (`HARDENING_REPORT.md`) are kept as written; this record states what was
done about H-8 and H-9 afterwards.

## H-9 — fullscreen presentations in the scale requirement

**Defect (on the integrated tree).** `Core::compute_rho` passed `fullscreen_output = None`, so
INV-SCALE-1 "the protocol-fullscreen presentation contributes `scale_O` (zoom 1)" was not wired.
Worse on the integrated tree: INC-7's `presentation_changed` rebuilt membership without any scale
evaluation, and a root commit evaluated before the presentation it started. A window granted
fullscreen on an output of another scale kept its windowed hint (at zoom 1 too).

**Change.**
- `compute_rho`: an output that presents an element (`Core::presentation`, OBJ-19, the predicate
  `view()` uses) contributes at zoom 1; any other at its camera's zoom. No new state.
- Fullscreen enter/exit is a non-client stimulus (C14.4): a request that starts or ends a
  presentation evaluates the policy.
- Root commit, tree change, destroy: the element's membership pass, then the presentation's, then
  one evaluation (INV-SCALE-5) — a client response unless a presentation started or ended.
- `presentation_membership` marks every element the touched outputs observed before or observe
  after: during a camera drag the evaluation is scoped, and only the marks make it visit hidden or
  revealed elements (found by the independent review; the first version relied on full passes).

**Tests (T0, product paths; persisted cameras put B elsewhere or at zoom 0.5).** Fullscreen enter
and exit; zoom-1 contribution at camera zoom 0.5; pre-map grant (hint before the initial
configure; B's scale at map); elements hidden by a presentation; restored at unset, unmap and
destroy; output reconfigure and removal; unmap and remap; a presentation change during a camera
drag equals the full reference; one evaluation per presentation batch; tree-change and root-commit
growth are client responses under SC-E; a presented element's commit releases no other element's
held hint. Five of the first tests fail on `2753b35` (run there by the implementer). Seeded
controls, all DETECTED: the seven named "H-9: …" in `tools/verify-test-efficacy`, and "SC-E: a
child surface's commit …" and "SC-E: a commit of a presented element …".

## H-8 — OB-28 test efficacy

`adversarial_client_churn_bounds_sc_e` passed with SC-E disabled: on one scale-1 output, ρ does not
depend on size, so no response could change the hint. Rewritten (same name): A (scale 1) and B
(scale 2, camera 1,500 px away); after one non-client stimulus the client grows onto B whenever its
hint is below 2 and shrinks off B at 2. With SC-E: exactly one change (`[240]`), never a decrease,
quiescent, no camera moved. With SC-E disabled: 30 alternating changes (`240, 132, …`). Control
"OB-28: SC-E disabled": DETECTED (the old test passes under the same seed). Lockstep only, no timing.

The test has one stimulus and no P7 or viewport-fit under the adversary: it is evidence for
INV-SCALE-4(d), not the whole OB-28 criterion.

## OB-42 — allocations

Attribution by backtrace (scratch copy of the test, not committed), on `f1ef262` (INC-6 before
INC-7), `23752f1` (INC-7, no INC-6) and this branch:

| Path | `f1ef262` | `23752f1` | this branch | source of the difference |
|---|---|---|---|---|
| Pan / PanCrossing / Route / Key / RenderList / FrameTick | 15 / 20 / 1 / 2 / 15 / 4 | same | same | none |
| Commit / CommitOffView | 9 / 4 | 10 / 5 | 10 / 5 | INC-7: `Core::presentations` snapshot per root commit (`f98309b`) |
| PresentedCommit (new path) | — | 24 | 24 | INC-7 presentation re-derivation; constant in the window count |

INC-6 adds no allocation on the measured paths; the harness adds none (only the Core call is
counted). Every remaining allocation is Core's own: temporary `Vec`s (`SurfaceTree::origins`,
`shown`, `views()`), membership rows, frame entries, `clock_output_of`, and the returned
`Vec<Effect>`. **The zero criterion is not met.** It needs C14.3's design: caller-owned reusable
effect buffers (18 public Core entries and 42 functions return `Vec<Effect>`) and per-output
buffers reused frame to frame. That is a Core API and adapter change, not an INC-6 repair.
Skipping the INC-7 snapshot when no grant exists would remove one allocation per commit but
touches ratified INC-7 comparison logic (the request paths start presentations from an empty grant
list) and does not reach zero; not done.

## C12.F9 — the unsafe boundary

A trial relocation (scratch, not committed) moved `t0_hot_path_allocations.rs` to
`crates/infynitae-backend-headless/tests/` unchanged: it compiles and passes there. With the Core
exception removed from `enforcement/source-rules` (one condition), `verify-architecture` fails
only its positive control for that exception. The relocation is: move the file; three inventory
paths; one condition and its constant; that control flipped to FAIL; two efficacy controls'
package; three knowledge records' paths. **Recommendation:** relocate and do not adopt C12.F9 —
INV-ARCH-2 then stays literal and no layer is needed. It reverses the owner decision of 2026-10-14
(allocator in a Core integration test), so it needs that decision revisited; and OB-42's "May be
closed by: T0 PRODUCT test in core" must then be read as "a T0 test of Core".

## OB-13 — on the integrated tree

The INC-6 evidence (`OB-13-EVIDENCE.md`) still holds: every cited test exists and passes. One gap
appears only with INC-7: a fullscreen grant is a size Core chooses, while `H_dev(E)` (F-SCALE-2)
uses the surfaces already committed. Probe (scratch): W seen by a scale-3 output, `D_max` 8192,
granted fullscreen on a 3840-px-wide scale-1 output: the configure is 3840×2160 and the standing
integer hint 3, so an obedient integer-scale client draws 11,520 px; P7 lowers the hint only after
that buffer is committed. The criterion "no buffer exceeds the limit Core was given" is not met for
that first frame. Remedy needs a reading of F-SCALE-2 (include a Core-configured size in `s_max`)
— an owner decision.

## Independent review (separate agent, 2026-10-15) — dispositions

| Finding | Disposition |
|---|---|
| (round 1) Two efficacy controls lost their anchors after the H-9 change (gate BROKEN) | fixed: re-anchored; all control anchors unique |
| Fullscreen enter/exit as non-client stimulus lets an unfocused client churn other clients' hints (probe: 40 changes in 20 create–grant–map–destroy cycles) | **open, owner decision**: C14.4 lists fullscreen enter/exit as non-client, but every such change is client-caused; C14.4's convergence argument does not hold for it |
| Presentation change during a drag not evaluated for affected elements | fixed (marks); test + control |
| Mutations survived: tree change as non-client; double evaluation; a presented element's commit as non-client | fixed: tests + controls (DETECTED). The third was first recorded here as unreachable; the re-review showed it releases the SC-E hold of *other* elements (a fullscreen client committing every frame would release every hold), so it got a test and a control too |
| Hint emitted after the configure on fullscreen enter/exit | open, owner decision (M15 orders only the initial hint) |
| Pre-map grant: two hints (canvas prediction, then B at map) | open, owner decision (F-SCALE-3 as written) |
| OB-42 "any path" wording; presented commit unmeasured; U-002 text | fixed: "measured paths"; PresentedCommit path (24); U-002 text |
| H-8/H-9 rows not updated | this record |

Re-review of `ce5029c` (same agent): the fixes for its round-1 findings 1, 3, 4 and 7 verified; it refuted this record's claim that the presented-commit mutation was unreachable (corrected above) and the control count (corrected above).
