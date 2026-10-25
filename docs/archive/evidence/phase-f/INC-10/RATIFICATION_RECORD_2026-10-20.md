# INC-10 — formal ratification record (2026-10-20)

Status: **RATIFIED 2026-10-20** by owner authorization 2026-10-20 (ratified commit `faf6dda`). This record takes effect when
the owner fast-forwards `main` to the commit that adds it; not pushed unless the owner pushes.
OB-24 stays OPEN (below, "OB-24").

## Authority

Owner authorization 2026-10-20: acceptance of INC-10's exit evidence through architecture change control
(the change-control record `evidence/phase-f/INC-10/CHANGE_CONTROL_RECORD_2026-10-20.md`, independently reviewed and re-checked 2026-10-20), with the
decisions below, and ratification of INC-10.

## Ratified commit

**`faf6dda`** (`faf6ddac1e2bce6a680d5149315a87b4154fea4b`): local `main` `56b3f12` (INC-10 implementation `b46021f` … `73c0aca`, M1
fix, the owner decisions of 2026-10-19, C12.F17 adopted 2026-10-20) and, on top of it,
`b594900` (test(runtime): the session-outlive test kills and reaps a leftover command; U-006 teardown recorded as a known limitation); `a56cc3c` (docs(inc-10): change-control record — exit criteria and blocking obligations mapped to evidence on 56b3f12); `632873c` (test(inc-10): change-control review — C12.F16.6 items 3, 6, 7, 10 as worded; OB-55 confinement case and lock point; TST-19 canvas-exact and TST-23 R1 ported); `c3fa23b` (docs(inc-10): change-control record corrected after the independent review — OB-24 does not block INC-10, its part 2 is not met as worded (U-1), closure needs a checker decision (U-8)); `eca016b` (docs(inc-10), test: change-control record after the review's re-check — closure path (A–G); item 7 compares the zoom bits); `d678d32` (docs(inc-10): gates on 56b3f12 all PASS (581/0/2, 431 seeds, 429/429 DETECTED, envverify, NB-1); perf_inc10 re-run on 56b3f12); `faf6dda` (docs(inc-10): change-control record — gates on the branch tip d678d32 all PASS (586/0/2, 433 seeds, 431/431 DETECTED, envverify, NB-1)). This record is a documentation-only commit on top of it.

## Verification before recording

The change-control record is the verification: full gates on `56b3f12` and on `faf6dda` (§2); the
exit tests, evidence items 1–11, negative controls and stop conditions mapped to production-path
tests and seeded controls, with the items it lists as open (§4, §6); the obligations (§5); an
independent adversarial review and its re-check (§7).

## Decisions of change control

Change control's decisions on the open items of the change-control record (§6), as given with the
owner's authorization:

- **U-1 (OB-24):** option (c) at this ratification — OB-24 stays OPEN; its closure is decided
  separately (an erratum layer, or a scoped closure with an explicit waiver and a forward
  obligation).
- **U-2 (TST-19 parity):** not INC-10 exit gaps. F4 (a held hint off the ladder after hotplug) is
  scale policy, INC-6's ratified scope, and is recorded as a parity item without a named production
  test; F7 (the popup constraint target, R8) at zoom ≠ 1 belongs to OB-14's open zoom ≠ 1 scope.
  F2 left with OB-53's CameraZoom part.
- **U-6 (F-LATTICE-1's residual sentence):** a forward reference to INC-10's exit, satisfied by
  item 2 on software renderers; its wording is corrected by the next layer that amends F-LATTICE-1.
- **U-9 (stale registry fields):** OB-24's "(items 1–10)" and OB-53's Item and "May be closed by"
  are corrected by the next amendment layer; not by this ratification.
- **Item 2:** the missing y-axis configuration (kept 2²⁰ reference below R) is accepted; the x-axis
  configuration and the symmetric code path cover it.
- **Risks:** FOUNDATION_RISKS is unchanged by this ratification (TC-1, TC-3, TA-1 as they stand).

## Scope

Zoomed navigation: one-shot `FitViewport` and restore (F-FIT-1 as amended by C12.F15 and C12.F17;
OBJ-30, 256 entries per output, C12.F16); zoomed pan with the output's kept lattice reference
(F-PAN-1, F-LATTICE-1, C12.F14); the canonical-rect render wrapper, crop and damage-tracker
invalidation (INV-RENDER-1, F-RENDER-1 as amended by C12.F15 and C12.F17); the fit/restore
bindings (M8, C12.F17.7); pointer reconciliation after camera actions, reconfiguration,
presentation start/end, interaction end and relocation (INV-INPUT-5, M10, C12.F17.6).
Software renderers only (Pixman, Smithay's test renderer); headless (T1/T0).

## Obligation dispositions

- **OB-24:** stays **OPEN** at this ratification. It does not block INC-10's exit (G-OB24 is met by
  C12.F14); its closure needs change control's decision on the change-control record's U-1 (the
  F-LATTICE-1 bound reset and the T3/T4 clause) and U-8 (the checker rule), and is a separate
  commit.
- **OB-53:** INC-10 part (the fit loop) met (`t1_fit_scale_adversary`); OB-53 stays **OPEN**
  (CameraZoom re-scoped; INC-14, INC-11 parts).
- **OB-14:** zoom ≠ 1 mismatch and clamp parts met (owner decision 2026-10-19); stays **OPEN**.
- OB-54, OB-55, OB-56, OB-57, OB-58, OB-59: **OPEN**, none blocks INC-10 (change-control record §5).

## Accepted residuals and limits

F-LATTICE-1's ±1 px re-rounding at each (re)definition of the reference and after a restart
(C12.F14; the re-rounding at the 2³¹ bound reset is not an accepted residual, change-control
record U-1); pass-through damage up to 1 px beyond the canonical rect (C12.F17.5); no GPU, GLES,
dmabuf, scanout or pacing evidence (OB-57, OB-58); frame times unpaced and without CPU isolation;
random trees, rotated outputs and multi-output spanning at zoom ≠ 1 untested (OB-14); no discrete
zoom stimulus exists (OB-53); drag-and-drop refused (OB-59).

## OB-24

OPEN. Its closure is a later, separate commit, generated only after change control decides U-1
((a) an erratum layer accepting the bound reset, or (b) a closure for Foundation 01's navigation
with an explicit owner waiver of the T3/T4 clause and a forward obligation on camera jumps) and
U-8 (the checker rule).
