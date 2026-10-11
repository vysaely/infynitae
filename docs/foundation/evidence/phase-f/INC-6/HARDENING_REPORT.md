# INC-6 hardening record

Status: **candidate, not ratified.** This file records what was changed after the independent audit of
`dev/inc-6-scale-policy` @ `5df5b15fc6dfc0085b9d897581b7e368136ecc61` (tree `3a0d7b82…`), why, and what
remains open. It is evidence, not authority. Verification results for a given commit are in the hand-over
message that accompanies it, not here (a file cannot record the verification of the tree that contains it).

- Base (audited candidate): `5df5b15`, kept as `recovery/inc-6-audited-candidate-5df5b15`.
- Hardening branch: `hardening/inc-6-scale-policy`.
- Audit under review: `INC-6_INDEPENDENT_AUDIT.md` (2026-10-11), verdict "AUDIT PASS", two Level C findings.
  The audit file is not in this repository; ingesting it under `docs/foundation/audit/` is an owner action.

Levels: A = architecture / authority / safety; B = material protocol, lifecycle or runtime defect;
C = minor, hygiene or latent.

## 1. Audit finding C1 — `preferred_scale_120` floor

**Root cause.** `scale.rs`: `(h * 120.0).round().max(120.0) as u32`. Every hint below 1.0 was sent as 1.0.

**Domain of h.** `h = t(ρ)` is a rung of `L(E)` or `H_max(E)` (F-SCALE-3), so `0 < h ≤ H_cfg`. Values below
1 are legitimate in two ways:

1. an output scale below 1 (`scale_120 > 0` is the only descriptor rule) is a rung (F-SCALE-2);
2. `H_max(E) = D_max / s_max(E) < 1` when a logical side exceeds `D_max` — the model's own EE-5 case
   `(side 20000, h 0.8192)`.

Camera zoom-out alone does not produce h < 1 while every output scale is ≥ 1: the ladder has no rung below
`min(1, output scales)`.

**Consequence.** In case 2 the floor told a fractional-scale client 1.0 where Core had decided 0.8192, so the
client rendered above the device limit (INV-SCALE-2). With `D_max = 4096` any side above 4096 triggers it.
That is a client-visible runtime defect, not hygiene: **reclassified B.**

**Protocol.** `wp_fractional_scale_v1.preferred_scale` is a `uint` numerator over 120. 0 is not a scale;
there is no other bound. So the only bounds that belong in the emission are those of the wire type.

**Correction.**

- `preferred_scale_120(h) = round(h·120)` bounded to `[1, u32::MAX]`.
- Invalid values are refused where they enter Core rather than hidden by the emission:
  `ScalePolicyConfig::validate` (`H_cfg` finite and > 0; `1 < r ≤ 2`; `0 ≤ σ < r − 1`; `T_settle > 0`;
  `D_max > 0`), applied by `Core::set_scale_config` and `Core::set_renderer_device_max`. Refusal keeps the
  state in force. With valid parameters h cannot leave `(0, H_cfg]` (test
  `c1_hint_domain_is_closed_under_valid_parameters`).
- An out-of-domain h is a Core defect: `debug_assert!` reports it in debug and test builds. In a release
  build the wire value is still at least 1. This is a residual design choice the owner may want changed to a
  `Result` (it would touch all eight emission sites).
- The audit's suggested patch (`.max(1.0)`) is equivalent for finite positive h; it did not address the
  parameters that can make h invalid (`D_max = 0`, non-finite `H_cfg`).

## 2. Audit finding C2 — scale hint dispatch

**Root cause.** `send_preferred_scale_to_window` sent `Effect::PreferredScale` to the toplevel root only.
Separately, the bind-time handler looked the surface up through Core's snapshot, which a surface is not part
of until it has committed.

**What clients saw (measured on `5df5b15` with the new tests).**

- A subsurface or popup never received a hint *change*.
- A surface that bound `wp_fractional_scale_v1` before its first commit — the usual toolkit order — received
  **no hint at all**. The audit's statement that "subsurfaces receive their initial scale at bind time" does
  not hold for that order.
- An integer-scale subsurface or popup was never sent `wl_surface.preferred_buffer_scale`, so it stayed at the
  protocol default 1 on a HiDPI output.

Blurry or mis-scaled menus, tooltips and decoration subsurfaces on any output whose scale is not 1:
**reclassified B.**

**The audit's suggested patch is unsafe.** It calls `send_preferred_scale_to_surface` (which calls
`with_states(surface)`) inside `Window::with_surfaces`. Smithay's traversal (`PrivateSurfaceData::map`) holds
that surface's `std::sync::Mutex` while it runs the callback, so the call self-deadlocks. Applied verbatim,
`initial_scale_hint_emitted_before_first_configure` hung and was killed by `timeout` (exit 124): the compositor
would stop on the first scale emission.

**Scope decided.** The hint is the element's (OBJ-16); the tree is the one the snapshot reports to Core and
`s_max(E)` ranges over: root tree and popup trees, nested popups included. `Window::with_surfaces` enumerates
exactly that set (root tree, then `PopupManager::popups_for_surface`), the same two calls `snapshot::extract`
uses. A surface outside the tree (another element, a destroyed `wl_subsurface` role, a dismissed popup) is not
enumerated.

**Correction.**

- A change is sent to every surface of the tree, using the `SurfaceData` the traversal hands out.
- A surface that joins later is told the element's current hint at the join event, with whatever subsurfaces
  it already has: `CompositorHandler::new_subsurface` and `XdgShellHandler::new_popup`. A popup therefore
  learns the hint before its initial configure.
- The `new_fractional_scale` body is removed. Every tree surface already holds the hint and Smithay sends the
  held value when the object is created; a second path would be a redundant fallback.
- Smithay sends each event only when it differs from what the surface was last told, so nothing is repeated.
- No policy in the adapter: it finds the owning element and forwards `Core::preferred_scale_of`.

## 3. Tests and negative controls

T0 (`infynitae-core`): `c1_preferred_scale_is_round_h_times_120_below_one`, `c1_preferred_scale_wire_bounds`,
`c1_out_of_domain_hint_is_reported`, `c1_wire_conversion_never_yields_zero`,
`c1_invalid_policy_parameters_are_rejected`,
`c1_hint_domain_is_closed_under_valid_parameters`, `scale_config_is_validated_at_the_core_boundary`.

T1 (`t1_scale`, production Core + adapter + headless backend, real `wayland-client`):

| Test | What it establishes |
|---|---|
| `c1_output_scale_below_one_reaches_the_client` | 0.75× output → `preferred_scale 90` |
| `c2_subsurface_tree_follows_scale_changes` | root, nested and integer-only subsurfaces across hotplug of a 1.5× output, the element leaving that output's view (CameraPan epoch: no decrease until release), and an output scale change; unmap/remap, role destruction, `wp_fractional_scale_v1` destruction, surface destruction: no stale event, no protocol error |
| `c2_late_surfaces_learn_their_own_elements_hint` | late subsurfaces, a pre-built subtree, a popup (hint before its initial configure), a late fractional-scale object; two elements with different hints (180 and 98) never cross |
| `c2_popup_trees_follow_scale_changes` | popup, popup subsurface and nested popup follow a change |
| `p7_forced_decrease_at_device_ceilings` | `D_max` 4096 / 8192 / 16384, against the last stimulus direction (SC-E alone would hold h) |
| `p7_applies_inside_an_open_epoch` | M7 `Open → Open{down}`; settlement keeps the bound |
| `p7_applies_to_an_unobserved_element` | P7 without an observer, by root commit and by subsurface commit |
| `device_ceiling_lowered_at_runtime_and_invalid_value_refused` | 16384 → 8192 → 4096 at runtime; `D_max = 0` refused, state kept |
| `integer_hint_follows_a_device_ceiling_change` | `b(h)` re-emitted when only `H_dev` changed |
| `initial_scale_hint_emitted_before_first_configure` (repaired) | both hints precede `xdg_surface.configure` on one ordered log |

Red before green: the two `c1_*` value tests failed on the unfixed emission (`left: 120, right: 90`); the two
original `c2_*` tests failed on the unfixed adapter (`S1: left [] right [120]`, `WS: left [] right [2]`).

Negative controls: `tools/verify-test-efficacy` had **no** INC-6 control. It now seeds 23, each of which must
make its named test fail: the audited C1 floor (T0 and T1); a zero wire value; `D_max = 0` and non-finite
`H_cfg` accepted; unvalidated `set_scale_config`; the audited C2 root-only dispatch; popup trees skipped;
join-time hint missing (subsurface ×2, popup, subtree); hint taken from the wrong element; configure before
the initial hint; `H_dev` ignoring `D_max`; sealed `⌈h⌉`; no P7 at batch-end evaluation (×2); no P7 on a
lowered `D_max`; no P7 for an unobserved element (root commit, child commit); `b(h)` not re-emitted; NaN
reaching the wire as 0.

## 4. Other defects found and corrected

| ID | Level | Finding | Correction |
|---|---|---|---|
| H-5 | C | The M15 order test searched `probe.log` for `toplevel_configure`, which is only ever written to `kb_log`, and fell back to `usize::MAX`. It passed with the configure emitted *before* the hint (verified by seeding that order). The audit's "wire event log confirms" rested on it. The implementation's order was in fact correct. | Test repaired; seed added |
| H-6 | C | Removing the P7 branch of `ScaleState::step` was detected by no test, T0 or T1 | Two discriminating T1 tests; seeds |
| H-7 | C | `set_renderer_device_max` emitted only when P7 lowered h; when only `b(h)` changed, an integer-scale client kept a buffer scale that no longer fitted | Emission compares with the last emitted pair; test; seed |
| H-13 | B | P7 was skipped for a mapped element that no output currently observes when its **root** surface grew past the device bound: `step` returns before its P7 branch when ρ is undefined, and `toplevel_committed` did not call `p7_check_element` (only the child-commit path did). The fractional hint stayed at 360 against `H_max` 2.048 while the integer hint dropped — INV-SCALE-2 says the decrease is immediate. Found by the independent reviewer. | `toplevel_committed` runs `p7_check_element`; `p7_applies_to_an_unobserved_element` (root and subsurface growth); two seeds |
| H-12 | C | Backends discarded the effects of `set_renderer_device_max`; comments cited OB-23 for `D_max` (it is OB-13); `f64::clamp` keeps NaN (→ 0 on the wire in release); `now + T_settle` could overflow | Corrected |

## 5. Open findings — not corrected here, owner decision required

| ID | Level | Finding | Why it is not fixed in this branch |
|---|---|---|---|
| **H-1** | **B** | **`round(h·120)` exceeds `H_dev`.** When h is device-bound (`h = D_max / s_max`), rounding to the nearest 1/120 goes up about half the time, and a fractional-scale client then renders above `D_max`: `D_max` 4096, side 3000 → 164/120 → 4100 px; 8192/3000 → 328 → 8200 px; 16384/8000 → 246 → 16400 px. The overshoot is at most `s_max / 240` px. OB-13's pass criterion is "no buffer exceeds the device limit". E1R-N3 bounded the integer hint only. The same family: the wire floor of 1 exceeds `H_dev` when `s_max > 240 · D_max` (reachable only through a `wp_viewport` destination). | The formula is canonical text (F-SCALE-3, INV-SCALE-5). Changing it — for example `min(round(h·120), ⌊H_dev·120⌋)` — is an architecture amendment under change control. The new T1 tests assert the canonical `round` values (164, 328, 246) and must be updated with the amendment. |
| **H-3** | **gate** | **OB-42 (INV-ALLOC-1) is unaddressed** and the plan lists it as blocking INC-6 exit. There is no counting-allocator test in the repository, and the visibility pass allocates per call (`output_scales`, `mapped_elements()`, per element `observers()` and the ladder). The audit's matrix does not mention OB-42. | Outside the two audited findings; a real implementation task (buffer reuse plus the T0 allocator test). |
| **H-4** | **gate** | **OB-13 says `D_max` is queried from the renderer.** Both backends pass the EC-16 constant 16384. See §6. | The brief for this hardening forbids inventing a GPU query; whether OB-13 can close on a fixture is the owner's call. |
| H-2 | C (latent B) | `Core::evaluate_scale_policy` uses `now_ms = 0`. Every visibility pass rewrites an open discrete epoch's deadline to `0 + T_settle`; opened at t = 10 000 ms it settles 200 ms early. Not user-visible today: the only production epoch is CameraPan (continuous, no deadline). | Needs a clock input to the batch-end evaluation — a Core entry-point change. Must be fixed before any discrete kind is wired. |
| H-14 | C | A **raised** `D_max` re-emits `b(h)` but does not re-evaluate h, and P7 has left the stimulus direction at "down", so client commits cannot raise it either: 16384 → 4096 → 16384 with side 3000 on a 3.0× output leaves the hint at 164 until an output event. | Whether a `D_max` change is a non-client stimulus (re-evaluate) or h is deliberately held is a policy statement M-renderer does not make ("possible P7" only). |
| H-8 | C | `adversarial_client_churn_bounds_sc_e` (the OB-28 T1 evidence) passes with SC-E disabled: its client never causes a scale change. T0 `tst17_feedback_model_and_wheel_wiggle` does detect it. | Test-evidence gap for OB-28, which also blocks INC-6 exit. |
| H-9 | C | `Core::compute_rho` always passes `fullscreen_output = None`; the fullscreen clause of INV-SCALE-1 is not wired and no test distinguishes it. The audit calls INV-SCALE-1 "production-reachable". | Protocol fullscreen is INC-7. Correctly deferred; the audit's wording is not. |
| H-10 | C | PROV-1's relation `H_cfg ≥ max connected output scale` is not checked (it changes with hotplug); `r` close to 1 is inside PROV-2 but makes the ladder arbitrarily large; `set_scale_config` has no production caller. | PROV-2 has no practical lower bound to enforce; configuration arrives in a later increment. |
| H-11 | C | The plan's named negative controls "1R camera-only rule (39)" and TST-17 "naive settle-every-time" are not present as controls. "Epoch disabled" and "no ladder" are detected by existing tests (checked by mutation, not seeded). TST-03 SC2 is represented on the production path only by hotplug and one pan. | Evidence gap, not a behaviour defect found. |

Audit record corrections: the audit reports 140 T1 tests (170 at `5df5b15`); "all 35+ seeded mutations
detected" involved no INC-6 mutation; OB-13, OB-28 and OB-42 — the obligations that block INC-6 exit — are
absent from its coverage matrix. "No Level B findings" does not survive: C1 and C2 were B, and H-1 is B.

`ScaleInteractionEpoch`: CameraPan is production-wired, as the audit says. CameraZoom, WindowMove,
WindowResize and ShellStream have Core state and a public `start_scale_interaction`, but no production caller,
and H-2 means the discrete ones would not behave correctly if wired today. "Deferred" is accurate;
"implemented in Core" needs that qualification.

## 6. Device capability boundary

- The boundary is `Core::set_renderer_device_max(d_max)` (backend → Core, OBJ-29 → F-SCALE-2). It is the only
  writer of `D_max` besides `set_scale_config`; both validate.
- `D_max = 16384` is EC-16: `maxFramebufferWidth` reported by `vulkaninfo` for one machine (RX 9070 XT / RADV).
  It is a recorded device fact used as a **provisional fixture**, now named `scale::DEFAULT_D_MAX`.
- Headless backend: there is no GPU to ask. The fixture is explicit and deterministic.
- Nested (winit / GLES) backend: **provisional, not discovered.** The host GPU's real `GL_MAX_TEXTURE_SIZE`
  may be lower than 16384, in which case Core would permit buffers the renderer cannot import. Smithay 0.7.0's
  `Renderer` trait exposes no maximum size, and reading it from GL needs `unsafe`, which the workspace denies
  (INV-ARCH-2). No query was invented.
- Exercised on the production path at `D_max` 4096, 8192 and 16384, plus runtime lowering and refusal of 0.
- The integer hint `b(h)` keeps an integer-scale buffer within `D_max` in every exercised case. The fractional
  hint does not (H-1).
- The pinned Smithay vendor patch is untouched.

## 7. Independent review

A separate reviewer context (same model family, no access to the author's reasoning, own private copy of the
tree, own experiments) reviewed commits `36ab4ce..af37861`: **APPROVE WITH FINDINGS**, no Level A, no defect in
the C1/C2 corrections. It independently reproduced H-1 (54,929 of 110,444 device-bound cases exceed `D_max`),
H-2, H-3 and H-4, and raised the items corrected in `0c7cdf2` and `16ab4cb` (NaN → 0, untested
`set_scale_config`, popup trees untested for changes, `b(h)` on a `D_max` change, a self-referential
assertion). A second pass over `8e5c96a..16ab4cb` again approved with findings: no regression, 20/20 seeds
detected by its own replay, and two further items — H-13 (corrected in the commit that follows) and H-14
(open). The commit that corrects H-13 and this record were **not** reviewed by it. Untested by its account:
`saturating_add` on epoch deadlines (no test distinguishes it from wrapping). It is a second AI context, not a human review, and does not replace the owner's independent
review gate.
