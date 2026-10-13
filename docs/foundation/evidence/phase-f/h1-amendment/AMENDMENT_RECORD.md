# H-1 amendment (registry layer C12.F2) — record

Status: **adopted 2026-10-13 by owner approval; local main, not pushed.** This file is evidence
and provenance. The authority is FOUNDATION_ARCHITECTURE §C12.F2.

## 1. What was approved, and what the layer says

Approved: "the corrected pre-client-rounding device-bound formulation".

    H_dev(E) = ⌊120 · D_max / s_max(E)⌋ / 120        s_max(E) > 0
             = +∞                                    s_max(E) = 0
             = 1/120 when the quotient is 0          (residual: nothing emittable fits)

| Document | Change |
|---|---|
| FOUNDATION_ARCHITECTURE §C12.F2 | new registry layer, one row (INV-SCALE-2); formula record C12.F2.2 |
| FOUNDATION_ARCHITECTURE §F F-SCALE-2 | first definition; `Amended` names C12.F2 |
| FOUNDATION_ARCHITECTURE PROV-1 | allowed-range note uses `H_dev(E)` |
| FOUNDATION_INVARIANTS INV-SCALE-2 | one clause, origin, layers, trigger |
| FOUNDATION_TEST_PLAN OB-13 | history line; status unchanged (OPEN) |
| `tools/check-foundation-docs` | 3 seeded fixtures; the layer-name check of `Amended` made exact |

F-SCALE-3 is not changed. The adopted text is the text of the reviewed proposal (A1c); nothing
was added to it.

## 2. The bound and the client's rounding are different things

The bound constrains the exact product `h · side`, before any client turns it into a buffer
size. It is what Core uses to choose the fractional hint. How a client then rounds (down, to
nearest as wp_fractional_scale_v1 asks, or up as some toolkits do) is the client's behaviour;
the compositor assumes none of them, and the bound is safe under all three. It is deliberately
not "the largest hint whose buffer fits": a client that rounds to nearest can fit one more step
on 376 / 383 / 430 of the sides 1…4·`D_max` at `D_max` 4096 / 8192 / 16384.

## 3. Independent verification of the formula (`h1_device_bound.py`, exact integers)

| Check | `D_max` 4096 | 8192 | 16384 |
|---|---|---|---|
| sides 1…4·`D_max` over the device, sealed bound, client rounding to nearest | 7,781 | 15,850 | 32,073 |
| same, adopted bound, client rounding down, to nearest or up | 0 | 0 | 0 |
| sides where the value is not the largest `w` with `s·w ≤ 120·D_max` | 0 | 0 | 0 |
| rounding to nearest instead of down, client rounding up: sides over | 8,109 | — | — |

Boundaries: `s = D_max` → 120/120; `D_max + 1` → 119/120; `120·D_max` → 1/120; one more → 0
(residual). `b(h)` unchanged: `⌊⌊120x⌋/120⌋ = ⌊x⌋` on every side. Floating point:
`round((w/120.0)·120) = w` for every `w` to 120·16384 and 400,000 samples to 120·(2³²−1).
Overflow: `120·D_max < 2³⁹`.

The first row counts every side, including those where `H_cfg` is the ceiling. Core's own count
(below) is restricted to sides where the device binds.

## 4. Core proof and regression suite

The scale policy is INC-6 work and is not on `main`. The implementation and its tests are on
the INC-6 experimental line (see CHECKPOINT.md for the commit). Oracles are integer arithmetic
written in the tests, not calls to `scale::h_dev`.

| Test | Establishes |
|---|---|
| `h1_no_emitted_hint_exceeds_the_device_at_any_side` | every side 1…4·`D_max` at three limits through Core: emitted value = `⌊120·D_max/s⌋` when the device binds; buffer fits under three rounding modes. 0 over on 15,019 + 30,038 + 60,075 bound sides (sealed: 7,300 / 14,739 / 29,639) |
| `h1_hand_derived_ceilings` | 163, 327, 245 by hand; 164, 328, 246 exceed |
| `h1_zero_and_boundary_sides` | no extent; `D_max`; `D_max`+1; 120·`D_max`; the residual; side 2³¹−1 |
| `h1_client_resizes_do_not_oscillate_the_hint` | 50-cycle resize alternation: 0 hint changes; a stimulus releases once |
| `h1_any_resize_sequence_keeps_the_buffer_within_the_device` | 20,000-step pseudo-random resize and stimulus script |
| `h1_fractional_output_scales_meet_the_bound` | outputs at 1.25, 1.5, 1.75, 2.25 |
| `h1_device_limit_transitions` | lowering is P7 at once; raising leaves the fractional hint held |
| `h1_device_bound_function_is_exact_and_total` | exactness in f64, monotonicity, ends of `u32` |

Seeded defects detected: sealed bound restored; rounding to nearest; rounding up; zero bound;
client-caused increase not held; 32-bit arithmetic.

## 5. Re-derived expectations

By hand, then compared with Core:

| Case | `120·D_max/s` | Adopted (down) | Buffer | Sealed (nearest) | Buffer |
|---|---|---|---|---|---|
| `D_max` 4096, side 3000 | 163.84 | 163 | 4075 | 164 | 4100 |
| `D_max` 8192, side 3000 | 327.68 | 327 | 8175 | 328 | 8200 |
| `D_max` 16384, side 8000 | 245.76 | 245 | 16333.3 | 246 | 16400 |

These three values occur at twelve places in four T1 tests of `t1_scale`
(`p7_forced_decrease_at_device_ceilings`, the device-limit test, `p7_applies_inside_an_open_epoch`,
`p7_applies_to_an_unobserved_element`) and in two assertions of the T0 test
`ee5_zero_extent_domain_and_integer_emission_bound` (2.048 → 245/120).

## 6. Not claimed

OB-13 is not closed. No real renderer's limit was queried: the headless backend supplies `D_max`
as a fixture. Draft A1 (a clamp at emission) was not built in Core; its oscillation is shown only
in a script model. The residual beyond 120·`D_max` is stated, not solved.
