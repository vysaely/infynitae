# Infynitae Native — Project Handoff

**This is a navigation aid, not an authority.** Every statement here summarises a canonical document in `docs/foundation/`. Where this file and a canonical document disagree, the canonical document wins, and this file should be corrected. Do not cite this file as normative.

State: Phase E **passed**; Phase F under way. INC-0, INC-1 and INC-2 ratified; INC-3 implemented and pending independent review; INC-4 has not begun. See `docs/foundation/CHECKPOINT.md`.

---

## 1. What Infynitae Native is

A native **spatial Wayland compositor** written in Rust on Smithay. Windows live on an unbounded 2-D **Canvas** (the Stage). Each output shows that Canvas through its own **camera** (centre + zoom). Users pan and zoom the camera, not the windows. Clients are ordinary Wayland clients; X11 apps run through `xwayland-satellite` as an ordinary Wayland client.

## 2. Architectural goal

One authoritative model of space, owned by a dependency-free core crate:
- **Stage** is the sole spatial authority (ADR-01). Smithay's `Space` is rejected.
- **Q16 fixed-point Canvas** with a reference-relative lattice, so rendering and hit-testing agree to the physical pixel (ADR-02, ADR-03).
- **Cameras** are changed only by trusted authority: physical input (`InputPort`), the user's shell (`ShellPort`), or output lifecycle (`OutputLifecycleToken`). Clients can never move a camera (ADR-04, ADR-08; INV-CAMERA-1).
- One unified scale policy with bounded hints and ScaleInteractionEpochs (ADR-05).
- Off-screen frame withholding and idle discipline (ADR-06, ADR-12).

Crates planned for Foundation 01 (FOUNDATION_ARCHITECTURE §C1.1): `infynitae-core` (zero dependencies), `infynitae-wayland` (Smithay adapter), `infynitae-backend-headless`, `infynitae-backend-winit`, `infynitae` (bin), `infynitae-ipc`; Phase G adds `backend-drm`. They exist under `crates/`; since INC-1 core, adapter and headless backend carry the first vertical seam (the others are still empty). Enforcement-only crates are under `enforcement/`.

## 3. Why a native compositor, not the old Hyprland plugin

The earlier Infynitae was a Hyprland plugin plus Quickshell UI. That is a separate legacy project and is not part of this repository. Phase A evidence (FOUNDATION_EVIDENCE §5, EV-H01…H07) shows what a plugin cannot fix:
- pan was O(N): every pan event moved every window (EV-H01);
- one camera bound to one "canvas monitor"; multi-monitor was detected, not modelled (EV-H02);
- identity was a stringified window pointer (EV-H03);
- `f64` canvas rounded at the host boundary (EV-H04).

A spatial model with per-output cameras, exact hit-testing and client-proof camera authority needs to own the compositor.

## 4–8. Phase history (Phases C and D)

| Phase | Commit | Outcome |
|---|---|---|
| A (evidence) | `2c8c163` | prototype and Hyprland-era evidence |
| B (decisions) | `4a0807e` | Phase B seal; decisions D-B…, Smithay quirks W-SM-* |
| **C (architecture)** | `615b0ed2f3631b698b0c54783c28f696ef4ff34a` | Foundation architecture sealed: crate DAG, Stage/camera/scale/hit models, authority spike (compile-time camera authority, asserted negative probes), C12 invariant registry |
| **D falsification baseline** | `e8ec03a2ab36f315ae025723ec666b9ad278ac83` | D1–D5 adversarial audits; findings PD-F2-01…19 |
| **D 1R** | `84ce620898445aaadb72f386ec3954e5bbfe2c2f` | one-shot viewport-fit with per-action `Authorized` proofs (PD-F2-01); ActiveElementFocus vs popup routing (PD-F2-02, INV-INPUT-10); constraint reconciliation deferred under grabs (PD-F2-03/04/05); scale epochs (PD-F2-07); xdg_toplevel request policy (PD-F2-17) |
| **D 2R** | `5827926782089dc8c6a1c203f844fbc879fce9be` | dedicated `OutputLifecycleToken`, hardware-only descriptors, passive persisted lookup (PD-F2-18); epochs for all spatial interactions (PD-F2-19) |
| **D final seal** | `c0a1cee310e56b75d32cf30152a09975ff776700` | D7 living package (one canonical home per namespace), `tools/check-foundation-docs`, D8 implementation plan, D9 readiness verdict |

## 9–11. Phase E

- **Initial Phase E review: FAIL** (`audit/phase-e/`). Two Level-A findings: E-F2-01 (rotated outputs double-swapped in Smithay's damage tracker) and E-F2-02 (Smithay `leave()` sends duplicate `unlocked`), plus Level B/C and checker findings.
- **E1R corrective checkpoint** (seal `1ae1b736515b8c52c3e6ba7596b22c81f7098390`): new amendment layer §C12.E1R.
  - physical mode `phys_O` (damage tracker) separated from the transformed view extent `view_O`;
  - hit-test fixed to `u = p · scale_O` (E1R-N1, a Level-A defect found during E1R);
  - constraint deactivation only from the active state, depending on Smithay patch **P1**;
  - `output_reconfigured`; zero-output `NoOutput` state; 1:1 camera binding; `OutputKey` collision rule;
  - zero-extent scale domain and device-bounded integer scale emission;
  - E-F2-07 (8K fullscreen buffer) disproved.
  - Independent re-review: `audit/phase-e-e1r-review/` (PASS).
- **Final Phase E review: `PHASE E PASS — PHASE F MAY BEGIN`** (`audit/phase-e-final/VERDICT.md`). Zero unresolved Level-A findings.

## 12. Important commits

| Meaning | Hash |
|---|---|
| Phase C seal | `615b0ed2f3631b698b0c54783c28f696ef4ff34a` |
| Phase D falsification baseline | `e8ec03a2ab36f315ae025723ec666b9ad278ac83` |
| Phase D 1R seal | `84ce620898445aaadb72f386ec3954e5bbfe2c2f` |
| Phase D 2R seal | `5827926782089dc8c6a1c203f844fbc879fce9be` |
| Phase D final seal | `c0a1cee310e56b75d32cf30152a09975ff776700` |
| Phase D tracking | `09075a2d07dbc0cba862baac62136f9f3ff9e7b5` |
| E1R seal | `1ae1b736515b8c52c3e6ba7596b22c81f7098390` |
| E1R tracking | `e9d36732ac4fa9e934ee1b90bb696e9545c598ab` |
| Final Phase E review recorded | `cfb54f091bd2a19065a45b32662e8f0bbddd62fc` |

The commit that adds this file comes after these; `git log` gives the current HEAD.

## 13–16. Registry sizes

- **57** effective invariants (39 C12, 9 C12.R1, 3 C12.R2, 6 C12.E1R) — FOUNDATION_INVARIANTS.md.
- **20** living formulas `F-*` — FOUNDATION_ARCHITECTURE §F.
- Obligations **OB-1 … OB-51** — FOUNDATION_TEST_PLAN.md.
- Increments **INC-0 … INC-17** — FOUNDATION_IMPLEMENTATION_PLAN.md.

## 17. Smithay 0.7.0

Exact pin `smithay = "=0.7.0"` (INV-ARCH-3, ADR-13). Workarounds are adapter-local; Smithay quirks are catalogued as W-SM-1…16 (FOUNDATION_BLOCKERS, living W-SM register). A dependency patch is allowed only under the patch policy: (1) reproduced, (2) not fixable through public API, (3) upstream issue/PR.

## 18. Smithay P1

One line in `src/wayland/seat/pointer.rs` `leave()`: deactivate a pointer constraint only if `is_active()`. Without it, Smithay 0.7.0 sends duplicate or unpaired `unlocked`/`unconfined` (2/11 lifecycle sequences valid; with P1 11/11). P1 exists **only** in the isolated experiment `experiments/e1r-smithay-p1/` (patch file + `build.sh`). The real pin is unchanged.

## 19–25. Gates and obligations that matter next

- **OB-11:** decide P1 vs upstream fix (criteria 1 and 2 met; criterion 3 — upstream issue/PR — open, an outward action for the owner). Must be decided before **any** production pointer-constraint behaviour ships (INC-5).
- **OB-19:** the **unconditional Phase F entry gate**. Port the spike's enforcement (dependency DAG, camera/token probes with asserted diagnostics, `!Send` checks, source rules) to the real crates in the first Phase F commit. Closed only by INC-0. Not waivable.
- **OB-24:** zoom anti-jitter (±1 px lattice residual while panning at zoom ≠ 1). Gate G-OB24 before INC-10; conservative fallback exists.
- **OB-34:** cross-output drag definition. Gate G-OB34 before INC-14.
- **OB-45 / OB-46:** rotated outputs — production T1 matrix (INC-2/INC-3 exits) / physical T4 validation (release claims).
- **OB-48 / G-OB48:** `OutputKey` persistence identity on real hardware; no cross-session camera persistence claim before it is decided.
- **G-MIRROR:** shared cameras (mirroring) are outside Foundation 01; binding is 1:1.

## 26. Physical validation debt

Nothing has been validated on the production adapter, a GPU, real toolkits, games or physical timing. Tiers T3–T5 (real clients, hardware, games/latency) and every PHYSICAL increment (INC-15/16/17) need separate authorization. See FOUNDATION_TEST_PLAN tiers and OB destinations T3/T4/T5.

## 27–28. INC-0

**Does:** creates the workspace skeleton and the enforcement before any behaviour — workspace manifests; `verify-architecture` ported from `experiments/c1-authority-spike/verify.sh` (probes 2d/2e/2f/2g plus E1R 2h/2i); CI jobs for `verify-architecture` and `tools/check-foundation-docs` (OB-36, a separate job); minimal type skeletons for ports, `OutputLifecycleToken` and `Authorized` only as the probes need them.

**Must not:** add product or Wayland protocol code; introduce authoritative state (no Stage; `CameraSet` exists only as a never-populated type shape, per the owner's INC-0 ruling recorded in `evidence/phase-f/INC-0/README.md`); touch any Wayland seam; change canonical architecture or audit records. Any probe that passes, or fails for the wrong reason, stops Phase F.

## 29. INC-1

The minimal real vertical seam: one element (root + subsurfaces + one popup) → public Smithay snapshot → core model → render list → DummyRenderer → physical-pixel hit-test; one output, one camera at zoom 1, via `output_connected`, no hotplug, no input routing, no scale policy. It tests whether Phase B–D harness results transfer to production boundaries. A failure stops work and returns to architecture change control.

## 30. Normative documents (canonical)

`docs/foundation/`:
- FOUNDATION_ARCHITECTURE.md — layers C12 / C12.R1 / C12.R2 / C12.E1R, namespace map §P0, formulas §F, provisional values §PROV
- FOUNDATION_INVARIANTS.md — effective `INV-*`
- FOUNDATION_DECISIONS.md — `ADR-*`
- FOUNDATION_STATE_MACHINES.md — `M1…M22`
- FOUNDATION_OBJECT_MODEL.md — `OBJ-*`
- FOUNDATION_TEST_PLAN.md — tiers, `OB-*`, tests
- FOUNDATION_IMPLEMENTATION_PLAN.md — `INC-*`, gates
- FOUNDATION_RISKS.md, FOUNDATION_EVIDENCE.md (§10 catalogue), FOUNDATION_BLOCKERS.md (PD-F register, W-SM register), PROTOTYPE_DECISION_LEDGER.md
- CHECKPOINT.md — current phase and next action

Only the `<!-- living:… -->` regions are current; text outside them is historical unless a pointer says otherwise.

## 31. Historical evidence (not current authority)

- `docs/foundation/audit/` — D1–D5, 1R, 2R, `phase-e/` (failed review), `E1R/`, `phase-e-e1r-review/`, `phase-e-final/`
- `docs/foundation/evidence/` — raw outputs per phase
- `experiments/` — harnesses, spikes and models (never product code)

## 32. Before touching code, read

1. `docs/development/START_HERE_FOR_AGENT.md`
2. `docs/foundation/CHECKPOINT.md`
3. FOUNDATION_IMPLEMENTATION_PLAN.md (INC-0, INC-1, gates)
4. FOUNDATION_TEST_PLAN.md (OB-19, OB-36)
5. FOUNDATION_INVARIANTS.md and FOUNDATION_ARCHITECTURE §C1, §P0, §F
6. FOUNDATION_DECISIONS.md (ADR-08, ADR-12, ADR-13)
7. `experiments/c1-authority-spike/` (what INC-0 must port)
8. `audit/phase-e-final/VERDICT.md`
