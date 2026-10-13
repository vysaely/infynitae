# OB-13 — evidence and disposition (INC-6, experimental)

Status: **OB-13 remains OPEN. It was not closed and cannot be closed as it stands.** This file
records what the INC-6 line proves, what it does not, and the decision that is needed.

## The ratified record (FOUNDATION_TEST_PLAN, verbatim in substance)

- **Item:** Runtime `D_max` query and enforcement. Test/decision: query from the renderer at
  startup; assert h ≤ H_max for large windows.
- **Pass criterion:** no buffer exceeds the device limit.
- **Destination:** T1. **May be closed by:** T1 production test (production core + adapter +
  headless backend). **Dependencies:** INC-6. **Blocks:** INC-6 exit.
- It is listed by two increments: INC-6 ("OB-13, incl. E1R zero-extent domain and integer
  bound") and INC-13 ("OB-13 (real `D_max`)").

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

## What it does not prove (the query half)

**No backend queries a renderer.** Both run loops hand Core the constant `DEFAULT_D_MAX`
(16384, the EC-16 device fact):

- headless: "an explicit, deterministic fixture, not a query";
- nested (winit/GLES): "PROVISIONAL … not a value discovered from this GLES renderer; the host
  GPU's real limit may be lower. Runtime discovery for the nested backend is not implemented".

So the first half of the item, "query from the renderer at startup", is unmet on every backend,
including the headless one that INC-6's own objective names ("`D_max` queried from the (Dummy)
renderer"). The windowless GLES test on the application-validation branch is experimental
evidence that a GPU context can be created headlessly; it queries no limit and is not renderer
ratification.

## Why it cannot simply be closed or split here

1. **Not all of it is met**, as above.
2. **It is one obligation shared by two increments.** Closing it at the INC-6 exit would also
   discharge INC-13's "OB-13 (real `D_max`)". Keeping the real-GPU part open means making it a
   separate obligation, which changes the sealed test plan and implementation plan.
3. **Obligations are closed at an increment's exit ratification**, after independent review
   (every closed record says so in its history). INC-6's exit is not authorized.

## The decision needed

Split OB-13 by amendment into:

- **OB-13a (INC-6, T0/T1):** enforcement — the hint and the integer hint never ask for a buffer
  above the limit handed to Core, with the zero-extent domain and the residual stated. Evidence
  above. Closable at the INC-6 exit.
- **OB-13b (INC-13, T2):** the limit is queried from the real renderer at startup and after
  renderer recreation, and enforcement is shown against that value on a real GPU.

and decide which of the two the Dummy renderer's "query" belongs to: either the headless backend
must ask its Dummy renderer for a (configurable) limit before INC-6 can exit, or a fixture
constant is accepted for INC-6 and the query is wholly INC-13's.

Until that is decided OB-13 stays OPEN and keeps blocking the INC-6 exit.
