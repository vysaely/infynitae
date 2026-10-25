# E1R — Finding dispositions

Status: **UNSEALED** (E1R review gate). The Phase E audit (`audit/phase-e/`) is unmodified evidence. Each Phase E finding has exactly one disposition. Evidence IDs resolve in FOUNDATION_EVIDENCE.md §10. Reproduction details: [REPRODUCTION_E1R.md](REPRODUCTION_E1R.md).

## Phase E findings

| Finding | Level (Phase E) | Reproduction | Disposition | Correction / destination | Evidence |
|---|---|---|---|---|---|
| E-F1-01 | E-F1 | **exact**: 47 bare tokens on 14 ADR lines, none validated | CONFIRMED — CORRECTED | ADR citations canonical `INV-*` (ranges expanded); checker `adr-unprefixed-inv`; fixture 13 | checker run + self-test |
| E-F2-01 | **A** | **exact**: source trace + executable matrix; the sealed rule fails every 90°/270°-class case | CONFIRMED — CORRECTED | `phys_O`/`view_O` split: F-VIEW-1, F-RENDER-1, F-LATTICE-1, F-MEMBER-1, F-PRESENT-1; INV-CAMERA-3, INV-OUTPUT-2 (descriptor carries physical mode + transform); OB-45, OB-46 | EE-1 (negative control: 7/7 rotated cases fail; corrected 10/10 pass) |
| E-F2-02 | **A** | **exact** on the wire, and **wider** than reported: also confinement, surface destruction and never-activated persistent locks (9/11 sequences invalid). The client-crash claim (SDL3/Chromium asserts) was **not** reproduced and is not relied on; the defect is judged against INV-INPUT-5/M13 ("exactly one", "lost/duplicate lock events") | CONFIRMED — CORRECTED | INV-INPUT-5 only-from-active; dependency patch P1 (validated) or upstream fix under OB-11 (criterion 3 open); M13, OBJ-21, ADR-07/13; W-SM-4c; OB-47; INC-5 decision now before any constraint ships | EE-2 (published Smithay 2/11 valid; P1 11/11), EE-7 |
| E-F2-03 | B | **exact**: no entry existed; M4 row 2 referenced one | CONFIRMED — CORRECTED | `output_reconfigured(&mut OutputLifecycleToken, OutputId, OutputDescriptor)`; F-RECONF-1; INV-OUTPUT-2; M4/M5/M10/M13; OBJ-10; OB-49 | EE-4 (3 tests; probes p10–p13; 2h with seeded negative control) |
| E-F2-04 | B | **exact**: INV-CAMERA-3 prose vs 1:1 data model; no binding API | CONFIRMED — CORRECTED | narrowed to 1:1; mirroring outside Foundation 01 (G-MIRROR) | EE-4 (1:1 collision test) |
| E-F2-05 | B | **exact** for the empty survivor set (sealed formula panics, N1); the Phase E fix (`Option` + "suspend relocation") is **partial**: it does not define the wire behaviour, and with a held client button the implicit grab keeps focus on a vanished output (N2), while Smithay's `unset_grab` leaks an absolute motion to a locked surface (N3) | CONFIRMED — CORRECTED | INV-OUTPUT-1 zero-output state with the zero-output grab (`Focus::Clear`); F-RELOCATE-1 domain; OBJ-20 `PointerState`; M4/M10/M13; W-SM-16; OB-50 | EE-3 (6 cases valid; N1–N3 fail), EE-4 |
| E-F2-06 | B | **partial**: `s_max = 0` is reachable at every initial commit; 0×N/N×0 are **not** singular (`s_max` is the max side); "integer division panic" depends on an implementation type, but the formula was undefined there in any arithmetic | CONFIRMED — CORRECTED | F-SCALE-2 domain via `H_dev` = supremum over an empty constraint set = +∞ → `H_max = H_cfg`; INV-SCALE-2 | EE-5 (negative control: sealed formula raises) |
| E-F2-07 | C | **disproved**: the canonical formulas give h = t(4.0) = H_max = 3.0 (H_cfg), buffer 5760×3240 ≤ `D_max`, not 7680×4320; the ×9 cost is the accepted RB-7 trade-off; the reviewer's clamp is CP2-Q1 (open policy) and would upsample ×4 on the observing output | DISPROVED WITH EVIDENCE | none; device-bound exception preserved; CP2-Q1 stays an open policy question | EE-6 |
| E-F3-01 | E-F3 | **exact**: Definition lines were not parsed | CONFIRMED — CORRECTED | checker: formula fields, structural syntax (closed code spans, balanced `{}`/`⟦⟧`, `(`/`[` closed by `)`/`]`), amendment provenance both ways; fixtures 14–16; boundary fixture 18 shows a well-formed wrong formula is **not** detected (not a theorem prover) | checker self-test 18/18 |
| E-F3-02 | E-F3 | **exact**: every experiment used `Transform::Normal` | CONFIRMED — CORRECTED | retained headless matrix EE-1 (Normal/90/180/270, flipped, 1.0/1.25/1.5, asymmetric 1920×1080); production T1 OB-45, physical T4 OB-46 | EE-1 |
| E-F3-03 | E-F3 | **exact**, plus a real defect found while testing it: on a concurrent key collision the sealed binding rule re-bound a camera already bound to another output (output left without a camera) | CONFIRMED — CORRECTED | identities separated (runtime `OutputId` / connector / physical monitor / persistence `OutputKey`); deterministic collision rule (INV-CAMERA-3); platform behaviour and persisting ambiguous keys: OB-48 + gate G-OB48 | EE-4 (test + `probe_sealed_key_steal` negative control) |

## Findings discovered during E1R

| ID | Level | Statement | Disposition | Evidence |
|---|---|---|---|---|
| E1R-N1 | **A** | F-HIT-1 `u = transform_O(p · scale_O)` tests a panel-space pixel against view-space rects; on every non-Normal output input reaches a surface other than the presented one (INV-HIT-1). Not part of E-F2-01's attacked claims. | CONFIRMED — CORRECTED (F-HIT-1 `u = p · scale_O`) | EE-1 C5: both readings of the sealed rule mismatch on 18/18 non-Normal cases; corrected 0/2000 |
| E1R-N2 | C | INV-INPUT-10 step 3 (`unset_grab`) can deliver one absolute motion at an unchanged location to a locked surface (mechanism W-SM-16). | VALIDATION OBLIGATION (OB-51) | EE-3 N3 (mechanism only) |
| E1R-N3 | B | Integer emission ⌈h⌉ can exceed the device bound for an integer-scale client (8000-px side: 24 000 px). | CONFIRMED — CORRECTED (F-SCALE-3 `b(h)`, INV-SCALE-5) | EE-5 (sealed exceeds in 2 cases) |

## Level-A accounting (no silent downgrade)

- E-F2-01: Level A kept; corrected with falsifying sequence (EE-1 matrix), corrected architecture passing, and the sealed rule failing.
- E-F2-02: Level A kept; corrected with falsifying sequences E1–E11 (EE-2), P1 passing, the published dependency failing. The correction depends on a dependency change that is not yet adopted in a pin (OB-11 criterion 3); INC-5 cannot ship constraints before it.
- E1R-N1: **new Level A** (classified upward under the ambiguity rule: hit delivery diverging from the presented surface is a Core/protocol divergence). Corrected with regression and negative control. **Under §26 of the E1R authorization a newly discovered Level-A defect blocks E1R**; this checkpoint therefore does not claim the E1R success condition on its own authority.
