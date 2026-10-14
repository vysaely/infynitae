# Infynitae Native — Engineering Knowledge System Index

**Location:** `docs/knowledge/`  
**Purpose:** Permanent, evidence-driven engineering memory preserving architectural discoveries, experiments, benchmark results, external research, and negative knowledge across development sessions.  
**Authority:** Non-authoritative engineering memory. Sealed governance lives exclusively in `docs/foundation/` (`CHECKPOINT.md`, `FOUNDATION_INVARIANTS.md`, `FOUNDATION_ARCHITECTURE.md`).

**State of the code these records describe:** main = origin/main = `f323c2d`. INC-0…INC-5 and INC-7 are RATIFIED; INC-6 is experimental and unratified; INC-8 and INC-14 have not begun (`docs/foundation/CHECKPOINT.md`). Records name the branch each fact holds on; a commit not on main is provenance, not current behaviour (see CONTRADICTIONS_AND_SUPERSEDED X-06).

**How to use this index:** read the task list in section 0, open only the records it names, then check `CHECKPOINT.md` and `FOUNDATION_INVARIANTS.md` for what is sealed. Kinds of knowledge are kept apart: `research/` (external systems), `decisions/` (what Infynitae decided and ratified), `architecture/` (implemented principles), `experiments/` (measurements), `pitfalls/` (rejected approaches), `roadmap/` (future work and residuals). Inside each record, "Unverified Hypotheses" holds what is not measured; the ledger holds every correction.

---

## 0. Start Here by Upcoming Work

| Work | Read first | Then |
| :--- | :--- | :--- |
| **INC-6** (scale policy; rebase onto `f323c2d`) | KNOW-ARCH-004 (main vs INC-6 split), KNOW-EXP-002, KNOW-EXP-005 | KNOW-PITFALL-005, KNOW-DEC-005 (R-15 converges with INC-6), KNOW-ROAD-005 |
| **INC-8** (frame callbacks, liveness) | KNOW-ROAD-005 (R-10), KNOW-PITFALL-006 | KNOW-RES-005, KNOW-RES-008 (heartbeat is a candidate; INV-FRAME-1 governs), KNOW-RES-009 (camera animation) |
| **INC-14** (interactive move/resize, grabs) | KNOW-PITFALL-007 (W-SM-19 → R-07 entry condition), KNOW-RES-003 | KNOW-RES-009 (hit-testing, live input under camera motion), KNOW-RES-010 (scrutinee lint, `with_states`, constraint snapshots), KNOW-DEC-004, KNOW-PITFALL-003 |
| **INC-13 / libinput** | KNOW-ARCH-002 (R-04, R-05) | KNOW-PITFALL-004 |
| **DRM/KMS backend (Phase G)** | KNOW-RES-008 (what transfers from Windows and what does not), KNOW-ROAD-001 | KNOW-RES-004, KNOW-RES-006, KNOW-RES-007, KNOW-RES-010 (scanout gating, fence wait), KNOW-EXP-004, KNOW-ROAD-002..004 |
| **Any DriftWM-inspired idea** | KNOW-RES-010 | the record its row names |
| **Fullscreen / presentation / focus changes** | KNOW-ARCH-001, KNOW-DEC-001, KNOW-DEC-002, KNOW-DEC-006 | KNOW-ARCH-003, KNOW-DEC-003, KNOW-PITFALL-001 |

---

## 1. Subsystem Index

### Core Architecture & State Machines
- [KNOW-ARCH-001: Camera-Independent Fullscreen Presentation Projection](architecture/KNOW-ARCH-001-camera-projection-independence.md) (`INV-PRES-1`, `INV-CAMERA-4`)
- [KNOW-ARCH-002: Monotonic Input Clock for Synthesized Pointer Events](architecture/KNOW-ARCH-002-monotonic-input-clock.md) (`INV-INPUT-5`, `M10`, Finding A-5)
- [KNOW-ARCH-003: Presentation Visibility Gating for Keyboard Focus](architecture/KNOW-ARCH-003-presentation-visibility-gating.md) (`INV-INPUT-8`, `INV-PRES-1`, Finding A-8)
- [KNOW-ARCH-004: Spatial Membership Culling and Scoped Scale Passes](architecture/KNOW-ARCH-004-spatial-membership-culling.md) (`F-MEMBER-1`, `INV-SCALE-1..5`, `OB-42`)

### External Research & Comparative Audits
- [KNOW-RES-001: DriftWM State Ownership and Space Bypass](research/KNOW-RES-001-driftwm-state-ownership.md) (Smithay abstraction analysis)
- [KNOW-RES-002: DriftWM Fullscreen Lifecycle and Restore Sizing](research/KNOW-RES-002-driftwm-fullscreen-lifecycle.md) (Saved size decoupling)
- [KNOW-RES-003: DriftWM Popup Grabs and Lock Reentrancy Workarounds](research/KNOW-RES-003-driftwm-popups-and-locks.md) (Smithay lock reentrancy)
- [KNOW-RES-004: Microsoft DirectFlip, Independent Flip, and Multi-Plane Overlays](research/KNOW-RES-004-msft-directflip-independent-flip-mpo.md) (DRM overlay plane scanout)
- [KNOW-RES-005: DXGI Waitable Swapchains, Queue Depth, and Frame Pacing](research/KNOW-RES-005-msft-dxgi-waitable-frame-pacing.md) (1-frame queue pacing)
- [KNOW-RES-006: WDDM Monitored Fences and Timeline Synchronization](research/KNOW-RES-006-msft-wddm-fences-timeline-sync.md) (Explicit timeline syncobjs)
- [KNOW-RES-007: Timeout Detection and Recovery (TDR) and Device Loss Handling](research/KNOW-RES-007-msft-tdr-device-loss-recovery.md) (GPU reset resilience)
- [KNOW-RES-008: Microsoft Graphics Research — Claim Classification and Transfer Limits](research/KNOW-RES-008-msft-research-claim-classification.md) (MS-DOC / INTERP / APPLY / WIN-ONLY; read before RES-004..007)
- [KNOW-RES-009: DriftWM Camera, Viewport, Coordinates and Hit-Testing](research/KNOW-RES-009-driftwm-camera-viewport-and-hit-testing.md) (coverage gap; comparison with INV-CAMERA/INV-COORD)
- [KNOW-RES-010: DriftWM Ideas — Disposition Register](research/KNOW-RES-010-driftwm-idea-disposition-register.md) (implemented / useful / rejected, one row per idea)

### Engineering Decisions & Protocol Bug Resolutions
- [KNOW-DEC-001: Pre-Map Fullscreen Request Ownership and Deferred Eviction](decisions/KNOW-DEC-001-a3-premap-fullscreen-ownership.md) (Finding A-3, `INV-PRES-1`)
- [KNOW-DEC-002: Rejection of Keyboard Focus on Occluded Windows](decisions/KNOW-DEC-002-a8-fullscreen-focus-eligibility.md) (Finding A-8, `INV-INPUT-8`)
- [KNOW-DEC-003: Default Placement After Fullscreen Acknowledgement](decisions/KNOW-DEC-003-p9b-default-placement-acknowledgement.md) (Finding P9b, `F-PLACE-1`)
- [KNOW-DEC-004: Popup Stacking Order and Smithay Traversal Defect](decisions/KNOW-DEC-004-p10-popup-stacking-w-sm-20.md) (W-SM-20, `X-15`, `OB-1`)
- [KNOW-DEC-005: INC-7 Integration Provenance (INC-7 Without INC-6)](decisions/KNOW-DEC-005-inc7-integration-provenance.md) (`integration/inc-7-exit-2026-10-14` → main)
- [KNOW-DEC-006: Unmap of a Fullscreen Element (C12.F10, RC-1)](decisions/KNOW-DEC-006-c12f10-unmap-of-a-fullscreen-element.md) (`INV-PRES-1`, M9)

### Experiments, Benchmarks & Validation
- [KNOW-EXP-001: Independent Reproduction of DriftWM Focus Pull Benchmark](experiments/KNOW-EXP-001-pull-cost-bench-reproduction.md) (Host reproduction)
- [KNOW-EXP-002: OB-42 Global Counting-Allocator Ratchet](experiments/KNOW-EXP-002-ob42-counting-allocator-ratchet.md) (`OB-42`, `U-002..U-004`)
- [KNOW-EXP-003: Automated Mutation Testing and Negative Controls](experiments/KNOW-EXP-003-mutation-testing-test-efficacy.md) (`verify-test-efficacy`)
- [KNOW-EXP-004: DRM Atomic Direct Scanout Validation Protocol](experiments/KNOW-EXP-004-drm-atomic-direct-scanout-validation.md) (Phase G validation)
- [KNOW-EXP-005: Core Benchmark Sensitivity to Codegen-Unit Partitioning](experiments/KNOW-EXP-005-benchmark-codegen-unit-sensitivity.md) (instruction counts before attribution)

### Pitfalls & Rejected Approaches (Negative Knowledge)
- [KNOW-PITFALL-001: Physical Canvas Fullscreen Relocation](pitfalls/KNOW-PITFALL-001-physical-canvas-fullscreen-relocation.md) (Rejection of canvas teleportation)
- [KNOW-PITFALL-002: Global Unconditional Focus Recalculation](pitfalls/KNOW-PITFALL-002-global-focus-pull-loops.md) (Rejection of focus pull loop)
- [KNOW-PITFALL-003: Permissive / Invalid Popup Grab Serial Tolerance](pitfalls/KNOW-PITFALL-003-bogus-grab-serial-tolerance.md) (Rejection of invalid serials)
- [KNOW-PITFALL-004: Mixed Timestamp Clock Domains](pitfalls/KNOW-PITFALL-004-mixed-timestamp-clock-domains.md) (Rejection of adapter wall-clock time)
- [KNOW-PITFALL-005: Quadratic Scale-State Evaluation](pitfalls/KNOW-PITFALL-005-quadratic-scale-state-evaluation.md) (Superseded pre-P1 defect)
- [KNOW-PITFALL-006: Unbounded Off-Screen Frame Callback Heartbeats](pitfalls/KNOW-PITFALL-006-unbounded-offscreen-callback-heartbeats.md) (Rejection of ungated heartbeats)
- [KNOW-PITFALL-007: Smithay 0.7.0 xdg-shell Limitations (W-SM-18, W-SM-19, W-SM-20)](pitfalls/KNOW-PITFALL-007-smithay-0-7-xdg-shell-limitations.md) (upstream fix 641d2ad; INC-14 guard)

### Future Architecture Roadmap
- [KNOW-ROAD-001: Phase G Hardware DRM/KMS Backend Blueprint](roadmap/KNOW-ROAD-001-phase-g-hardware-drm-backend.md) (Bare-metal udev backend)
- [KNOW-ROAD-002: Explicit Timeline Synchronization with linux-drm-syncobj-v1](roadmap/KNOW-ROAD-002-explicit-timeline-sync-syncobj.md) (Explicit sync)
- [KNOW-ROAD-003: Variable Refresh Rate (VRR) and Adaptive Frame Pacing](roadmap/KNOW-ROAD-003-vrr-adaptive-sync-pacing.md) (Adaptive sync)
- [KNOW-ROAD-004: DRM Damage Clips and Panel Self Refresh (PSR) Optimization](roadmap/KNOW-ROAD-004-fb-damage-clips-psr.md) (Laptop battery optimization)
- [KNOW-ROAD-005: Work Remaining After the INC-7 Exit Preparation](roadmap/KNOW-ROAD-005-inc7-follow-up-work.md) (ratification outcome, residuals R-01…R-15 by owner)

---

## 2. Meta Documents & System Governance
- [README.md](README.md): System architecture, schema rules, and metadata definitions.
- [CONTRADICTIONS_AND_SUPERSEDED.md](CONTRADICTIONS_AND_SUPERSEDED.md): Traceability ledger of disputed, superseded, and corrected findings.
- [PROVENANCE_INVENTORY.md](PROVENANCE_INVENTORY.md): Comprehensive inventory of original sources, laboratory reports, and external commits.
- [AGENT_INTEGRATION.md](AGENT_INTEGRATION.md): Proposed integration additions for agent instructions (`AGENTS.md`, `CLAUDE.md`).
- [WORKFLOW.md](WORKFLOW.md): Standard engineering development and knowledge maintenance workflow.

---

## 3. Quick Reference by Status

| Status | Knowledge Records |
| :--- | :--- |
| **IMPLEMENTED on main, INC-7 RATIFIED 2026-10-14** | KNOW-ARCH-001, KNOW-ARCH-002, KNOW-ARCH-003, KNOW-DEC-001, KNOW-DEC-002, KNOW-DEC-003, KNOW-DEC-004, KNOW-DEC-005, KNOW-DEC-006, KNOW-PITFALL-007 |
| **IMPLEMENTED, split main / INC-6** | KNOW-ARCH-004 (visibility on main; scale scoping INC-6 only) |
| **IMPLEMENTED on INC-6 lines only (unratified)** | KNOW-EXP-002 |
| **VALIDATED** | KNOW-EXP-003 |
| **RESEARCH / PROPOSED** | KNOW-EXP-005, KNOW-RES-001…KNOW-RES-010, KNOW-EXP-004, KNOW-ROAD-001…KNOW-ROAD-004 |
| **REJECTED** | KNOW-EXP-001, KNOW-PITFALL-001, KNOW-PITFALL-002 (evidence DISPUTED, X-08), KNOW-PITFALL-003, KNOW-PITFALL-004, KNOW-PITFALL-006 |
| **DEFERRED** | KNOW-ROAD-005 |
| **SUPERSEDED** | KNOW-PITFALL-005 (Superseded by KNOW-ARCH-004) |
