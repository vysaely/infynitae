# OB-13 — evidence (INC-6, experimental)

Status: **OB-13 is OPEN.** It was split by the owner on 2026-10-14 (record on `main`:
`docs/archive/evidence/phase-f/ob-13-split/RECORD.md`). This file records what the INC-6
line shows for the INC-6 half. The split did not close anything: OB-13 closes at the INC-6 exit
review, and **OB-52 (the real renderer's limit on a GPU, INC-13, T2) is open and untouched**.

## The obligation as split

- **OB-13 (INC-6, T1):** Core enforces the reported device limit; the backend obtains that limit
  through the renderer interface; a headless fixture may report a documented deterministic
  limit; no unexplained constant is passed directly into Core. Pass criterion: no buffer
  exceeds the limit Core was given.
- **OB-52 (INC-13, T2):** the limit is queried from the real renderer, re-queried when the
  renderer is recreated, and enforcement is shown against that value on a GPU. No fixture
  result counts.

## What the INC-6 line proves (the enforcement half)

| Requirement | Evidence | Tier |
|---|---|---|
| h ≤ H_max for large windows; P7 lowers at once | `p7_forced_decrease_at_device_ceilings`, `p7_applies_inside_an_open_epoch`, `p7_applies_to_an_unobserved_element` | T1, production core + adapter + headless backend, a real client |
| the limit handed to Core is the one enforced, and a change of it is enforced at once | the device-limit test in `t1_scale` (16384 → 8192 → 4096; 0 refused) | T1 |
| no fractional hint asks for a buffer over the limit (C12.F2) | `h1_no_emitted_hint_exceeds_the_device_at_any_side` (every side to 4·`D_max` at three limits; 0 over on 105,132 bound sides), `h1_any_resize_sequence_keeps_the_buffer_within_the_device` | T0 |
| integer hint within the limit (E1R-N3) | `ee5_zero_extent_domain_and_integer_emission_bound`, `c1_hint_domain_is_closed_under_valid_parameters` | T0 |
| zero-extent domain (E-F2-06) | same; `h1_zero_and_boundary_sides` | T0 |

Stated residual, not a failure of the criterion as amended: a logical side above 120·`D_max`
cannot fit at any emittable scale.

## The capability path (added 2026-10-14)

| Requirement of the split | Implementation | Evidence |
|---|---|---|
| The backend obtains the limit through the renderer interface | `infynitae_wayland::limits::RendererLimits`; `WaylandState::report_renderer_limits` asks the renderer, hands the answer to Core and applies Core's effects | T1 `t1_device_limit` (5 tests) |
| No constant is passed directly into Core | `Core::set_renderer_device_max` may be named only in `infynitae-wayland::limits`; source rule `device-limit-only-through-renderer-limits`, with two seeded violations in `tools/verify-architecture` | architecture gate |
| A headless fixture may report a documented deterministic limit | `HeadlessRenderer` (Smithay's test renderer plus a `DeviceLimit`); default 16384 = EC-16, documented at `HEADLESS_REFERENCE_DEVICE_LIMIT`; configurable through `HeadlessCompositorOptions::device_limit` | `the_ceiling_at_startup_is_what_the_renderer_reports` |
| Limit changes | a changed limit takes effect when reported, not before, and then at once (16384 → 8192 → 4096: hints 360 → 327 → 163, integer hint 3 → 2 → 1; raised again: still fits) | `a_changed_limit_is_enforced_when_reported` |
| Boundary values | `u32::MAX`, 9000 and 8999, 6000, 3001 and 3000 for a 3000-pixel side, each with hand-derived hints | `boundary_limits` |
| Error handling | a failing query and a limit of 0 leave Core's ceiling and the client's hints untouched; the headless session does not start without a usable limit | `a_failed_query_and_a_refused_limit_change_nothing`, `the_session_does_not_start_without_a_device_limit` |

Six seeded controls cover the path (constant instead of the reported value; fallback on a
failed query; the run loop ignoring its option; the run loop ignoring the result; a fixture
that reports the default whatever it was given; a test harness that does not ask).

## What is not shown, stated plainly

- **Nothing asks a GPU.** The headless renderer has no device. The nested (winit/GLES) backend
  reports through the same interface from a type named `AssumedGlesLimits`, which returns the
  EC-16 value and says in its documentation that it is not a query. On a host whose real limit
  is lower, a buffer Core allowed can fail to import. That is OB-52.
- **Core has a ceiling before anyone reports one**: `ScalePolicyConfig::default()` carries
  `DEFAULT_D_MAX` (16384). Both run loops and the test harness report before serving a client,
  so no client is served under the unreported default; a future backend that forgot to report
  would run on it silently. Making the unreported state explicit in Core would change Core's
  configuration type and was not done here.
- A logical side above 120·`D_max` cannot fit at any emittable scale (stated residual of
  C12.F2).

## What remains before OB-13 can close

The INC-6 exit review. The requirements of the INC-6 half are met on this experimental branch
by the evidence above; closing the record is the owner's act at that review.
