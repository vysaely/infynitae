# INC-6 exit — review round r2 (2026-10-16)

Status: **EVIDENCE, not authority. Nothing here adopts, ratifies or closes anything.** Branch
`candidate/inc-6-exit-r2-2026-10-16`, from `candidate/inc-6-exit-2026-10-16` (`b784b55`).
Produced under the owner brief of 2026-10-16 ("INC-6 exit, INC-8 review and continuous runtime
development"), which approves the *direction* of C12.F11 and C12.F12, approves C12.F13
conditionally on verification of its final semantics and test evidence, allows C12.F9 to be
withdrawn after its dependants are checked, asks for OB-28 to be split, and authorizes preparation
and validation only, not ratification.

The texts ready for adoption are in [ADOPTION_PACKAGE_2026-10-16.md](ADOPTION_PACKAGE_2026-10-16.md).

## 1. Commit `4c81a1c` against the proposal texts

`4c81a1c` changed the behaviour of two proposals after they were written and did not update their
texts. Both texts are now corrected to the code (marked "Correction of 2026-10-16" in each
proposal; no behaviour change):

| Proposal | Old text | Code (unchanged since `4c81a1c`) | Verdict |
|---|---|---|---|
| C12.F11 | "The saved size of an unset … is not counted" | the size offered at fullscreen exit counts in `s_max(E)` until E commits a tree of another size, is unmapped (C12.F10 drops it) or destroyed; a re-grant before the redraw saves the size still on offer | code is right: the old text overflows `D_max` when the window is larger than the output (3000 px offered at hint 2, `D_max` 4096) |
| C12.F13 | "Any camera drag releases every hold … the owner may prefer to limit release to stimuli that touch the element" | only a non-client stimulus that touches the held element releases it: an output change touches every element; a camera action touches the elements an output bound to the moved camera observed before or observes after it; a settlement does not lower a held element its stimulus never touched; P7 always applies | the narrower option, which the proposal itself offered |

## 2. Adversarial tests added in r2 (each fails without the rule it tests)

| Test (T0, `infynitae-core --lib`) | Attack | Seeded control |
|---|---|---|
| `a_spent_restore_offer_no_longer_bounds_the_hint` | does the offer become a permanent cap? (redraw at 3000, shrink to 300, A rescaled to 2: hint must reach 2) | "C12.F11: a redraw at another size does not spend the offer" |
| `an_unmap_spends_the_restore_offer` | does a stale offer survive unmap/remap? | "C12.F11: an unmap keeps the size on offer" |
| `a_re_grant_before_the_redraw_offers_the_windowed_size_again` | set → unset → set → unset before any redraw: is the window offered its fullscreen geometry? | "C12.F11: a re-grant saves the fullscreen geometry instead of the size on offer" |
| `an_output_connected_releases_every_hold` | does a system change that never observes the held element release it (it must: outputs touch everything)? | existing "C12.F13: an output change does not touch every element" |
| `a_hold_dies_with_its_element` | does a hold leak to a new element? | (covered by P-1 `no scale state outlives its element`) |

All five pass on r2. The three new controls are DETECTED (run individually; the full efficacy run
is in §6).

## 3. Obligations, one by one

### OB-13 (device limit, INC-6 half)
- **Criterion:** no buffer exceeds the reported device limit; the limit reaches Core only through
  the renderer capability interface; a change of the limit, the boundary values and an invalid
  limit are handled.
- **Evidence on r2:** the grant (`a_grant_never_asks_for_a_buffer_over_the_device_limit`), the
  pre-map grant (`a_pre_map_grant_initial_hint_fits_the_device_limit`), the restore offer
  (`the_size_offered_at_fullscreen_exit_fits_the_device_limit` and the three r2 tests), P7 on a
  held window (`p7_lowers_a_held_window`), the renderer-capability path (`40ddd7d`, source rule).
- **Depends on C12.F11 being adopted:** without it the configured fullscreen size and the restore
  offer are outside the canonical `s_max(E)`, and the code would contradict F-SCALE-2.
- **Residual (unchanged, F-SCALE-3):** a client whose own logical side exceeds `D_max` at scale 1
  cannot fit; carried by OB-13's text and by OB-52 (real renderer, INC-13, T2).
- **Recommendation:** closable at the INC-6 exit **after** C12.F11 is adopted.

### OB-28 (scale feedback adversary)
- **Criterion:** with an adversarial T1 client, hint changes ≤ rungs crossed + 1 + one P7 per
  non-client stimulus, per epoch; the run ends quiescent; no camera mutation from client geometry.
- **Covered on r2 (T1, each with a seeded control):** SC-E after a non-client stimulus
  (`adversarial_client_churn_bounds_sc_e`), P7 (`adversarial_client_under_p7_changes_the_hint_once`),
  a continuous pan epoch (`adversarial_client_inside_a_pan_epoch`); cross-client presentation churn
  0 (C12.F13, T0 `a_client_cycling_fullscreen_does_not_churn_another_clients_hint` and probes A–C).
- **Not coverable now:** the EC-22 viewport-fit loop (INV-CAMERA-5 has no code before INC-10)
  and the discrete CameraZoom / WindowMove / WindowResize / ShellStream epochs (INC-10, INC-14,
  INC-11). These stimuli do not exist in the product yet, so no client can drive them.
- **Recommendation:** split (texts in the package): OB-28 keeps the stimuli that exist and closes
  at the exit; **OB-53** (new) carries the remainder, destinations INC-10 (fit loop, CameraZoom),
  INC-14 (WindowMove/Resize) and INC-11 (ShellStream), T1, same criterion. The union is the
  original; nothing is dropped.

### OB-42 (hot-path allocations, INV-ALLOC-1)
- **Criterion:** zero heap allocations in Core hot paths after warm-up (T0 counting global
  allocator, test-only). Measured: 0 on all nine paths at 10, 100 and 1,000 windows; the test
  asserts 0; six seeded reintroductions detected; a source rule (`b784b55`) keeps production on
  the measured caller-buffer entries.
- **Placement / interpretation finding (r2):** the registry row says "May be closed by: **T0
  PRODUCT test in core**". Since the 2026-10-15 relocation (owner direction; INV-ARCH-2 forbids
  `unsafe` in every Core target) the test lives in `crates/infynitae-backend-headless/tests/`. It
  calls only `infynitae_core` entries (no adapter, no Smithay), so it is T0 by content and
  measures Core's hot paths; it is not "in core" by location. The closure record must say which
  reading the owner accepts. The package proposes the wording "T0 PRODUCT test of Core's entries
  (a test binary outside Core may hold the counting allocator; INV-ARCH-2)". Without that, closing
  OB-42 would rest on an unstated reinterpretation.
- **Also corrected:** KNOW-EXP-002 still described the file as a ratchet with non-zero bounds;
  it now records that the file asserts zero (history kept).
- **Scope limit (unchanged):** steady state only. A batch that emits a scale hint or settles an
  epoch, a constraint change, a binding's action and a button press still return a `Vec`
  (`FINAL_RESOLUTION_2026-10-15.md`). Not measured: a pan while a presentation holds other
  elements (the hold branch allocates nothing by inspection; no measurement).
- **Recommendation:** closable at the exit with the wording above.

### OB-44 (C14 baselines)
Needs physical measurements (T4); not an INC-6 exit condition. Stays OPEN.

## 4. C12.F9 — dependants checked

`grep` over the tree (docs, tools, crates) for `C12.F9`/`C12F9`: CHECKPOINT (deferral note),
FOUNDATION_INVARIANTS (number reserved), FOUNDATION_ARCHITECTURE §C12.F10 (number reserved),
UNSAFE_INVENTORY (history: not adopted), the INC-6 evidence records, and three knowledge records.
No invariant, formula, obligation, checker rule, test or control depends on C12.F9: INV-ARCH-2 is
unchanged and enforced as written (`verify-architecture`: Core forbids `unsafe` in every target).
The only obligation touched is OB-42, through the allocator's location (§3).
**Withdrawal is safe.** Text in the package.

## 5. Independent adversarial review of the candidate (r2 reviewer's notes)

- **Authority and lifecycle invariants:** F11 reads stage/fullscreen/offer state only; no new
  writer of anchors, cameras, z-order or focus. The offer lives in `restore_offers`, cleared on
  redraw at another size, on `drop_fullscreen` (unmap, destroy) and replaced on every exit. F13
  changes only which evaluations may lower a hint. INV-STAGE-1, INV-CAMERA-1, INV-PRES-1 hold
  (existing T0/T1 suites pass).
- **Fullscreen scale (H-9):** `compute_rho` counts the presenting output at zoom 1 and drops the
  hidden camera presentations (`a_presenting_output_contributes_its_scale_at_zoom_one`, the H-9
  tests) — unchanged in r2.
- **Camera-hold release:** see §1 and §2. One behaviour worth the owner's attention: a held
  element hidden by another client's fullscreen keeps its higher hint across camera drags on
  outputs that never see it, until an output change or a drag that touches it. This is memory,
  never sharpness, and it is what "touched only" means.
- **No defect found in r2.** Three new controls added; nothing in Core's production code changed.

## 6. Gates on r2

Run 2026-10-16 on the r2 tree (`b784b55` + this round's changes): `cargo fmt --check` PASS,
`cargo clippy --workspace --all-targets --locked -D warnings` PASS, `cargo test --workspace
--locked` PASS (441 passed, 0 failed), `verify-architecture` PASS, `check-foundation-docs` and
its self-test PASS, `verify-knowledge --content-only` PASS, `verify-test-efficacy` PASS (341
seeded defects detected, including the three added here). All headless (T0/T1); nothing here is
hardware or runtime evidence.

## 7. What remains, and whose it is

1. Owner: adopt the package (C12.F11, C12.F12, C12.F13 with corrected texts; record C12.F9
   withdrawn; OB-28 split into OB-28 + OB-53; OB-42 closure wording).
2. Mechanical, after adoption: apply the package on a governance branch from this candidate,
   add the checker's `F_LAYERS` entries and fixtures, drop the "REHEARSAL / NEVER MERGE" labels
   in code comments and control names, re-run every gate.
3. Owner: ratify INC-6 and fast-forward `main`. Not done here.

## 8. Round r3 (2026-10-16, third brief): texts re-verified line by line

- **Defect found and fixed (C12.F13).** `evaluate_scale_policy` took "every element is touched"
  from the sweep's `all` flag. `all` is also set by `set_renderer_device_max` and
  `set_scale_config`, which only ask for a full re-evaluation and run P7 themselves. The report
  released no hold, but the *next* non-client stimulus — a camera drag on an output that never
  saw the held element — released every hold. Reproduced by
  `a_device_limit_report_releases_no_hold_now_or_later` (failed before the fix). Fix: a separate
  `Sweep::system` flag, set only by the visibility pass of an output change. Seeded control "a
  full re-evaluation (device limit, configuration) is read as touching every hold": DETECTED; the
  older control "an output change does not touch every element" still detected.
- **C12.F12 text** overstated the code ("exactly the presentations that observe the element at
  its first map"); the package now states the geometric rule the code applies.
- **C12.F11 text** now says which redraw spends the offer: a tree whose window-geometry size
  differs from the one it replaces.
- **OB-42:** `verify-architecture` gained `check_ob42_core_only` (the allocator test names no
  crate but `std` and `infynitae_core`), with two seeded violations, both caught. The proposed
  wording (package P-6) states that requirement instead of the location "in core".
- **C12.F9:** re-checked; no new dependant.
