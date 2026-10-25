# C12.F15 — owner decision record (2026-10-17)

Status: **owner decisions 1–5 RECORDED; layer C12.F15 ADOPTED on local `main` (base `dda4d87`).
Governance and documentation only. Not pushed. OB-24 stays OPEN. INC-10 is NOT started.**

## 1. Authority

Owner brief "Ratify C12.F15 and Record C12.F14 Integration" (2026-10-17), approving:
1. Record that C12.F14 is integrated on local `main` and G-OB24 is satisfied, provided the
   repository state verifies it — recorded in CHECKPOINT ("LAYER C12.F14 INTEGRATED"), commit `dda4d87`.
2. Option B: one shared canonical rectangle for rendering and hit testing.
3. Amend INV-RENDER-1 and F-RENDER-1 to establish the canonical-rectangle rendering contract.
4. The corrected F-FIT-1 unit rule and clamp.
5. Single-step restore: each successful fit records the camera value immediately preceding it; one
   restore reverses the most recent fit; repeated restores unwind earlier fits.

These approvals cover the governance layer and its record commits only — not INC-10, not the
production wrapper, not a push. Preceding analysis: the read-only review of `270128f` and the
C12.F15 design pass (2026-10-17), with the prototype evidence in this directory (`PROVENANCE.md`).

## 2. Verification before writing

| Item | Verified against | Result |
|---|---|---|
| Integration | refs, reflog, gate log | `main` = `270128f` (fast-forward from `7ddde8f`); `origin/main` = `23752f1`; gate tree `8137291c` = `270128f^{tree}` |
| Draw destination | `vendor/smithay-0.7.0` `damage/mod.rs:381,429`, `element/surface.rs:383-401` | the tracker draws each element at `geometry()`; the surface element renders into the given `dst` — the wrapper governs placement and pixels (Pixman run) |
| F-FIT-1 units | F-VIEW-1 (`v_O = centre − view_O/(2k_O)`; `view_O` physical px), INV-COORD-1 (Q16, ≤ 2³⁶ px), geometry anchor (ADR-03) | Canvas extent `view_O/k_O` → zoom = quotient / `scale_O`, scale applied once; window geometry integral → centre exact in Q16; domain constants `ZOOM_MIN` 2⁻²⁰, `ZOOM_MAX` 2¹⁶ (`geom.rs`); no fit code exists yet |
| Restore | INV-CAMERA-5, OBJ-30, M8 | INV-CAMERA-5's wording admits a history; OBJ-30's single value ("until the next fit") is amended; cameras are 1:1 with outputs (E1R) |
| Pointer | M10 view-change row; `entry.rs` true-position guard | "reconcile once by the same rule" = one batch-end reconciliation, ≤ 1 true-position event: exactly one with the pointer on O and no interaction |
| Q16 budget | `projection.rs` `camera_on_grid`; F-LATTICE-1 transitions; F-VIEW-1 | 3·2⁻²² < δ under the stated assumptions (C12.F15.7) |
| Crop | `CropRenderElement` (`element/utils/elements.rs`); PD-F1-01; F6 | consistent for buffers without a transform; other transforms unverified |

## 3. What the layer changes

FOUNDATION_ARCHITECTURE §C12.F15 (new) and living F-RENDER-1, F-FIT-1; FOUNDATION_INVARIANTS
INV-RENDER-1 (origin C12.F15), layer list, count; FOUNDATION_OBJECT_MODEL OBJ-30;
FOUNDATION_STATE_MACHINES M8; FOUNDATION_DECISIONS ADR-02, ADR-04; FOUNDATION_IMPLEMENTATION_PLAN
INC-10 (entry prerequisites and exit evidence); FOUNDATION_TEST_PLAN OB-24 history; CHECKPOINT;
`tools/check-foundation-docs` (`F_LAYERS`; the E1R unchanged-list check scoped to E1R's own records;
fixtures 44, 45; fixture 16 re-anchored). C12.F14 is unchanged; its two errata are in C12.F15.6.

## 4. Open after this record

- **Rotated and flipped buffer transforms** in the crop: unverified; an INC-10 exit requirement.
- **History depth bound** (OBJ-30): not decided; an owner decision before INC-10 ships restore.
- GLES / GPU behaviour of the wrapper: not exercised (software renderers only).
- OB-24: OPEN until architecture change control accepts INC-10's exit evidence.
