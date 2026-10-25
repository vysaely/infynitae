# Phase F Increment INC-1 Independent Adversarial Review Dossier

## 1. Review Overview

- **Increment Reviewed:** Phase F, Increment 1 (INC-1) — Minimal Real Vertical Seam
- **Reviewed HEAD:** `8f44f8be4a067e38f4231c1ee781267d4584d1ea`
- **Reviewed Baseline:** `a3710e1bf5c478e228af64bb1e10cf6e4d67d497` (INC-0 Ratification Baseline)
- **Branch:** `main` (clean working tree, tracking `origin/main`)
- **Remote:** `https://github.com/vysaely/infynitae.git`
- **Execution Class:** HEADLESS, non-destructive, offline
- **Review Role:** Independent Principal Systems Architect, Senior Rust Engineer, and Senior Wayland/Smithay Compositor Engineer
- **Verdict:** `INC-1 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION`

## 2. Dossier Structure

This dossier contains the independent findings, proofs, and evaluations of Increment INC-1:

1. [`VERDICT.md`](VERDICT.md) — Formal independent verdict, justification, conditions for ratification, and authorized next actions.
2. [`RECONSTRUCTION.md`](RECONSTRUCTION.md) — Independent reconstruction of canonical requirements, living architecture references, and compliance matrix.
3. [`SMITHAY_PARITY.md`](SMITHAY_PARITY.md) — Comprehensive technical analysis of pinned Smithay 0.7.0 behavior, coordinate formulas, render elements, and parity proofs.
4. [`ADVERSARIAL_ATTACKS.md`](ADVERSARIAL_ATTACKS.md) — Methodology, test cases, and results of independent adversarial probes, boundary pixel attacks, and expanded mutation testing.
5. [`OBLIGATION_REVIEW.md`](OBLIGATION_REVIEW.md) — Adjudication of architectural obligations `OB-2` and `OB-26`, and tracking of all open obligations (`OB-1`, `OB-6`, `OB-14`, `OB-17`, `OB-49`).

## 3. Executive Summary of Findings

1. **Architectural Soundness (Level A: 0 findings):**
   - The production authority model (`Core::new`, `Authorized::mint`, private ports/tokens) remains uncompromised.
   - Pinned Smithay 0.7.0 behavior (`with_surface_tree_downward`, `WaylandSurfaceRenderElement::from_surface`) is accurately integrated without constructing `Space` or invoking `Window::surface_under`.
   - The single-origin rule (`F-ORIGIN-1`) and coordinate transformations (`F-VIEW-1`, `F-LATTICE-1`, `F-RENDER-1`, `F-HIT-1`, `F-UNPROJECT-1`, `F-PROJECT-1`) hold strictly with zero sub-pixel drift across tested scales and coordinates up to $2^{36}$.
   - All workspace crates strictly enforce safe code (`#![forbid(unsafe_code)]` in core/ipc, `#![deny(unsafe_code)]` elsewhere, 0 unsafe blocks).
   - Single-output boundary is respected; INC-2 multi-output/hotplug code has not been prematurely introduced.

2. **Test Sensitivity & Oracle Independence (Level B: 2 findings):**
   - **`INC1-F06` (Level B):** Test oracle in T1 (`tst10_xhit` and `vertical_slice_hit_test_equals_presented_surface`) calls production `view.unproject(p)` to compute `q` for region eligibility. Seeded mutation `ADV-MUT-3` (swapping x/y in `unproject`) passed `tst10_xhit` due to this shared helper dependency.
   - **`INC1-F07` (Level B):** `render::entries_for` correctly skips unshown surfaces (`if !shown { continue; }`), but no existing test in the suite exercises `render_list` on an element with an unmapped/unshown subsurface. Seeded mutation `ADV-MUT-2` (removing the check) passed `tst09_xsnap`.

3. **Prior Findings Adjudication:**
   - `INC1-F01` (Level C): AGREE. Default maximize/fullscreen handlers are kept from Smithay; owned by INC-7 (M2).
   - `INC1-F02` (Observation): AGREE. SeatState held without seat or `wl_seat` global; input intake is INC-3/INC-4.
   - `INC1-F03` (Observation): AGREE. Camera center anchor is a valid Level D choice for INC-1.
   - `INC1-F04` (Observation): AGREE. Refusal of second output and opaque `OutputKey` properly preserve INC-2 boundary (OB-49).
   - `INC1-F05` (Observation): AGREE. Explicit view parameters on pure functions properly test fractional cameras and $2^{36}$ ranges.

4. **Obligations Disposition:**
   - `OB-2` (Input region evaluation): MECHANICALLY SATISFIED. Ready for ratification upon resolution of conditions.
   - `OB-26` (Smithay parity regression guard): MECHANICALLY SATISFIED. Ready for ratification upon resolution of conditions.
