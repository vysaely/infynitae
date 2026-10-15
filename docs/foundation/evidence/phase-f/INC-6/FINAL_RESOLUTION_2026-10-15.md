# INC-6 final engineering resolution — record (2026-10-15)

Status: evidence, not authority. Branch `integration/inc-6-final-resolution-2026-10-15`, from the
exit-hardening branch @ `3666286`. **INC-6 is not ratified; no obligation is closed; nothing is
merged or published.** Owner direction of 2026-10-15, items 1–5, each checked against the
canonical rules first. Builds on `EXIT_HARDENING_2026-10-15.md`. Amendments the directions need are
rehearsed on `rehearsal/inc-6-scale-amendments-2026-10-15` (C12.F11, C12.F12, C12.F13 on top of
this branch; never merge before adoption). An earlier rehearsal of C12.F11/F12 only,
`rehearsal/inc-6-scale-amendments-c12f11-f12` @ `43369a5`, is superseded and kept.

## Policy decisions and governance

| Owner item | On this branch | Governance |
|---|---|---|
| 1. A client's fullscreen must not move unrelated clients' hints | **stopped; proposed as C12.F13; rehearsed** | contradicts C14.4 (client response = the client's own geometry commit; fullscreen enter/exit a non-client stimulus) and INV-SCALE-5 (MaxGesture at zoom 1). A first implementation here (`28421bc`) treated it as an interpretation; the independent review showed both the contradiction and two defects (a one-batch hold that an active client or a second presentation still churned; a revealed window left too low). Reverted at `546bf83` to the canonical behaviour; corrected design in `C12F13_PROPOSAL.md` |
| 2. The device bound counts a Core-configured size | **stopped; proposed as C12.F11; rehearsed** | changes F-SCALE-2's `s_max` |
| 3. Scale hint before the configure on fullscreen entry and exit | **implemented** (also for an output reconfiguration that resizes a presented element) | no living rule orders them (M15 orders only the initial hint); `wp_fractional_scale_v1.preferred_scale` and `wl_surface.preferred_buffer_scale` are independent of the xdg configure sequence |
| 4. A pre-map grant's initial hint from the granted output | **stopped; proposed as C12.F12; rehearsed** | changes F-SCALE-3 / INV-SCALE-5's initial hint |
| 5. Relocate the OB-42 allocator instead of C12.F9 | **implemented** | INV-ARCH-2 unchanged; the 2026-10-14 owner decision needs its formal correction (text in `C12F9_PROPOSAL.md`) |

A conflict avoided in item B: `INV-CAMERA-1` names `physical_input` as the only input entry where
a proof may be minted, so `physical_input` itself takes the caller's `EffectBuf` (C14.3's
refinement) and the allocating form is `physical_input_vec` (forbidden in the adapter).

## Item 3 — ordering

Batches that start or end a presentation (grant, unset, a commit that maps a pre-map grant or
replaces a presentation) and an output reconfiguration put every scale hint before their first
configure; stable, in place. Tests: T0 effect order (grant, unset, replacement, reconfiguration);
T1 wire order with a real client (`preferred_scale` and `preferred_buffer_scale` before the
`xdg_surface.configure`) for entry and exit.

## Item 5 — unsafe boundary

`t0_hot_path_allocations.rs` moved unchanged (pure rename) to the headless backend's tests; the
Core integration-test exception removed from `enforcement/source-rules`; its positive control now
expects FAIL. Fidelity: the per-path counts at 10, 100 and 1,000 windows were identical in both
locations (27 values compared) before the zero-allocation work.

## OB-42 — zero allocations (workstream B)

Measured, debug build, every round after warm-up, n ∈ {10, 100, 1000}; one scale-1 output, the
measured window has an explicit input region:

| Path | `3666286` | this branch |
|---|---|---|
| Pan step / crossing pan step | 15 / 20 | **0 / 0** |
| Routed motion / key press+release (no binding) | 1 / 2 | **0 / 0** |
| Commit / off-view commit / commit of a fullscreen window | 10 / 5 / 24 | **0 / 0 / 0** |
| Render list / frame tick | 15 / 4 | **0 / 0** |

Boundary: the entries the adapter uses (`physical_input`, `toplevel_committed_into` — the adapter
calls its `_acked` twin, the same code — `render_list`, `frame_tick_into`) with caller-owned
buffers created once; only the Core call is counted (trees are built before it). Six seeded
reintroductions of an allocation are detected by the test.

Scope, stated plainly (C14.3 "steady state"): these paths still allocate when they produce
something beyond the measured steady state — a batch whose scale evaluation emits a hint or settles
an epoch (`evaluate_scale_policy`, `settle_*` return a `Vec`), a constraint change
(`reconcile_constraints`), a key binding's action (`apply_key_action`), a button press. None of
these occurs per frame or per event in a steady pan, route, key, commit or render; each would need
the same buffer conversion to be zero too.

Design per C14.3 (no object pools): caller-owned `EffectBuf` (append-only; grows by one allocation
only when an event yields more effects than ever before; nothing dropped); per-element tree
layouts in the Stage (out of line, allocated once at creation); one non-allocating projection
visitor; membership rebuilt in a reused spare table by merging sorted runs; per-output frame
slots invalidated in place; reused scratch for views, rows, presentation snapshots and touched
outputs; `InputRegion::Ops(Arc<[RegionOp]>)`; exact integer cull thresholds (`View::cull`).
Adapters migrated (`WaylandState::with_effects`, `WaylandState::frame_tick`, the commit handler).

Latency, `core_bench`, release, 3,000 windows, one output, median of three (`0fb2c66` → `6e46962`):
pan 28.8 → 15.5 µs, crossing pan 28.2 → 15.3, render list 125 → 54, frame tick 0.12 → 0.05 (the
reviewer reproduced these within a few per cent on `5aa04d0`); remap 48 → 40 and hotplug 800 → 690
here, but not reproduced by the reviewer (its runs, concurrent with gate runs, gave ~50 and
~771), so only "not slower" is claimed for them; commit and off-view commit within run-to-run noise
(0.4–1.0 µs). The first version was 17 % slower on pan;
the cause was measured (identical rounding work, 9,085 `edge` calls per pan, moved by codegen) and
removed by the integer cull, equal to `may_show` on 80,000 random cases and at extreme scale
factors (bounded search).

## Other defects fixed on this branch

- F-SCALE-3 prediction compared the px anchor with Q16 view edges (INC-6, `cbac5e5`): a far
  window could predict an output near the origin and renegotiate at map (`546bf83`, with a test).
- `View::cull`'s threshold search could step ~10^12 times for a non-finite estimate; now bounded.

Recorded, not changed (pre-existing on main): a camera zoom below about 1e-11 (e.g. 1e-12; a
persisted camera may carry one, `PersistedCameras` checks only "finite, > 0") makes `View::new`
overflow: `view/(2k)` saturates the Q16 view origin (debug panic, release wrap). 1e-6 to 1e-11
are fine (reviewer's probe).

## OB-13

The INC-6 evidence holds on the integrated tree except the first fullscreen frame: with an observer
of higher scale, a grant's configure and the hint in force can ask for a buffer over `D_max`
(measured: 11,520 px for 8192). Not met until C12.F11 (or another remedy) is adopted. On the
rehearsal branch the first buffer fits, the hint lowered in the grant's batch before the configure.

## OB-28

Covered now, each with a seeded control: SC-E after a non-client stimulus; P7 under an adversary;
an adversary inside a continuous pan epoch; no camera mutation from client geometry. Not coverable
yet: the viewport-fit loop of EC-22 (INC-10, whose entry needs INC-6's exit) and discrete
CameraZoom / WindowMove / WindowResize / ShellStream epochs (INC-10, INC-14, INC-11). Closing
OB-28 at the INC-6 exit needs an owner decision on that remainder (a split, as OB-13 → OB-52).

## Independent review (separate agent)

Round 1 (`5aa04d0`): verified correct — hint ordering (a stable rotation moving only hints;
delivery needs no prior `enter`), `EffectBuf`/`with_effects` (nothing lost, duplicated or
reordered), the integer cull, the membership merge, `TreeLayout`, frame staleness, the
authority and unsafe rules, the zero-allocation assertion at every size. Findings and dispositions:

| Finding | Disposition |
|---|---|
| Item 1's hold lasts one batch: an active client or a second presentation still churns | item 1 moved to C12.F13 with a persistent hold (rehearsal) |
| A revealed window can be left too low (regression vs `3666286`) | gone with the revert; C12.F13 raises revealed windows as non-client |
| Item 1's "clarification" is an amendment (INV-SCALE-5, C14.4) | accepted: C12.F13 |
| Prediction unit bug (px vs Q16) | fixed, test, control |
| Output reconfiguration: configure before hint | fixed, test, control |
| Surviving mutants: destroy as non-client; P7 for held elements | destruction control restored (DETECTED); the held-P7 case moves with item 1 to the rehearsal |
| Probe p3 imprecise | fixed |
| Record and comment inaccuracies (`physical_input_into`, adapter commit entry, U-002, OB-42 scope, commit latency) | corrected here and in the test file |
| Cull search could effectively hang | bounded |

Round 2 (`b5afa89`, `ef6c97e`): the dispositions above verified (probes A, B, C, D, E re-run:
churn 10 / raised / 10 on this branch, 0 / raised / 0 on the rehearsal; controls anchored and
unique). New: on the rehearsal, C12.F13's hold was never ended once unneeded (probe F) and one
mutant survived (a subject keeping its hold) — fixed there (`30e33bf`), with targeted tests;
the overflow threshold and the latency claim above corrected. The full planted-defect run then
showed seven controls that still mutated the reference projection (`entries_for`) or a match arm
the zero-allocation work had changed (M1): re-anchored on the production code, all DETECTED.
