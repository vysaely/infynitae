# INC-6 final engineering resolution — record (2026-10-15)

Status: evidence, not authority. Branch `integration/inc-6-final-resolution-2026-10-15`, from the
exit-hardening branch @ `3666286`. **INC-6 is not ratified; no obligation is closed; nothing is
merged or published.** Owner direction of 2026-10-15, items 1–5, each checked against the
canonical rules first. Builds on `EXIT_HARDENING_2026-10-15.md`.

## Policy decisions and governance

| Owner item | Outcome | Governance |
|---|---|---|
| 1. A client's fullscreen must not release unrelated clients' held hints | **implemented** | C14.4 lists "fullscreen enter/exit" as a non-client stimulus; read as the stimulus of the element whose own presentation started or ended (the subject). Membership changes of other elements caused by a presentation are not on C14.4's list (only camera or Stage movement). A clarifying sentence is proposed below for confirmation. M9's "settlement" at a grant is applied to the subject's epoch. |
| 2. The device bound counts a Core-configured size | **stopped; proposed as C12.F11; rehearsed** | changes F-SCALE-2's `s_max` (canonical) |
| 3. Scale hint before the configure on fullscreen entry and exit | **implemented** | no living rule orders them (M15 orders only the initial hint); `wp_fractional_scale_v1.preferred_scale` and `wl_surface.preferred_buffer_scale` are independent of the xdg configure sequence |
| 4. A pre-map grant's initial hint from the granted output | **stopped; proposed as C12.F12; rehearsed** | changes F-SCALE-3 / INV-SCALE-5's initial hint (canonical) |
| 5. Relocate the OB-42 allocator instead of C12.F9 | **implemented** | INV-ARCH-2 unchanged; the 2026-10-14 owner decision needs its formal correction (text in `C12F9_PROPOSAL.md`) |

One more conflict was found and avoided during item B: `INV-CAMERA-1` names `physical_input` as the
only input entry where a proof may be minted. A buffer-taking `physical_input_into` would have
broken that wording, so `physical_input` itself takes the caller's `EffectBuf` (as C14.3's
refinement says) and the allocating convenience form is `physical_input_vec`.

Proposed clarification for item 1 (C14.4 / INV-SCALE-4, no behaviour beyond the implementation):
"Fullscreen enter/exit is a non-client stimulus for the element whose presentation starts or
ends. An element whose observers change only because another client's presentation started or
ended is evaluated as a client response and its hint is not lowered by that change; a change the
system makes (output added, removed, reconfigured) is a non-client stimulus for every element."

## Item 1 — request source vs resulting state

- Subjects (`Sweep::stimulate`): open epoch settled (M9), evaluated as a non-client stimulus.
- Every other element a client's presentation change touches: client response, and held against a
  decrease (`Sweep::hold_decreases`); it may still rise, so a revealed window is never left
  under-resolved by the change. P7 still applies.
- System-initiated changes: unchanged (full non-client pass).
- Evidence: an unfocused client cycling create / pre-map grant / map / destroy 20 times plus a
  focused client toggling 20 times change another window's hint **0** times (**80** on `3666286`).
  Tests: hold, reveal raises, system-ended presentation moves unrelated hints, subject's own
  fullscreen is a non-client stimulus (its last direction was down), M9 settlement during a drag.

## Item 3 — ordering

Batches that start or end a presentation (grant, unset, a commit that maps a pre-map grant or
replaces a presentation) put every scale hint before their first configure; stable, in place.
T0 effect order for grant, unset and replacement; T1 wire order (a real client: `preferred_scale`
and `preferred_buffer_scale` before the `xdg_surface.configure`) for entry and exit.

## Item 5 — unsafe boundary

`t0_hot_path_allocations.rs` moved unchanged (pure rename) to the headless backend's tests; the
Core integration-test exception removed from `enforcement/source-rules`; its positive control now
expects FAIL. Fidelity: the per-path counts at 10, 100 and 1,000 windows were identical in both
locations (27 lines compared) before the zero-allocation work.

## OB-42 — zero allocations (workstream B)

Measured, debug build, every round after warm-up, n ∈ {10, 100, 1000}:

| Path | `3666286` | this branch |
|---|---|---|
| Pan step / crossing pan step | 15 / 20 | **0 / 0** |
| Routed motion / key press+release | 1 / 2 | **0 / 0** |
| Commit / off-view commit / commit of a fullscreen window | 10 / 5 / 24 | **0 / 0 / 0** |
| Render list / frame tick | 15 / 4 | **0 / 0** |

The measured window has an explicit input region. Boundary: the entries the adapter uses
(`physical_input`, `toplevel_committed_into`, `render_list`, `frame_tick_into`) with
caller-owned buffers created once; only the Core call is counted (trees are built before it).
Design per C14.3 (no object pools): caller-owned `EffectBuf` (append-only; grows by one allocation
only when an event yields more effects than ever before; nothing dropped); per-element tree
layouts in the Stage (out of line, allocated once at creation); one non-allocating projection
visitor; membership rebuilt in a reused spare table by merging sorted runs; per-output frame
slots invalidated in place; reused scratch for views, rows, presentation snapshots and touched
outputs; `InputRegion::Ops(Arc<[RegionOp]>)`. Adapters migrated (`WaylandState::with_effects`,
`WaylandState::frame_tick`, the commit handler). Six seeded reintroductions of an allocation are
detected by the test.

Latency, `core_bench`, release, 3,000 windows, one output, median of three (`0fb2c66` → this):
pan 28.8 → 15.5 µs, crossing pan 28.2 → 15.3, render list 125 → 54, commit 0.56 → 0.47, frame
tick 0.12 → 0.05, remap 48 → 40, hotplug 800 → 690; off-view commit within noise (0.39–0.52). The
first version was 17 % slower on pan; the cause was measured (identical rounding work, 9,085
`edge` calls per pan, moved by codegen) and removed by exact integer cull thresholds
(`View::cull`, equal to `may_show` on 80,000 random cases).

## OB-13

The INC-6 evidence holds on the integrated tree except the first fullscreen frame (above): not met
until C12.F11 (or another remedy) is adopted. On the rehearsal branch the first buffer fits
(integer and fractional), the hint is lowered in the grant's batch, before the configure.

## OB-28

Covered now, each with a seeded control: SC-E after a non-client stimulus; P7 under an adversary;
an adversary inside a continuous pan epoch; no camera mutation from client geometry. Not coverable
yet: the viewport-fit loop of EC-22 (INC-10, whose entry needs INC-6's exit) and discrete
CameraZoom / WindowMove / WindowResize / ShellStream epochs (INC-10, INC-14, INC-11). Closing
OB-28 at the INC-6 exit needs an owner decision on that remainder (a split, as OB-13 → OB-52).

## Independent review

(Filled in after the review.)
