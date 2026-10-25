# Phase E Final Review — Implementation Plan Audit

**Review Date:** 2026-10-05  
**Auditor:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  

---

## 1. Incremental Plan Architecture Overview

The 18 implementation increments (`INC-0` through `INC-17`) in `FOUNDATION_IMPLEMENTATION_PLAN.md` structure the construction of Infynitae Native into testable vertical slices with explicit execution classes and gate dependencies:

| Increment | Category | Execution Class | Core Deliverables | Entry Gate |
|---|---|---|---|---|
| **INC-0** | ENFORCE | HEADLESS | Workspace manifests, `verify-architecture`, CI enforcement | Phase F authorized |
| **INC-1** | PRODUCT | HEADLESS | Single-window minimal slice: snapshot $\to$ core $\to$ DummyRenderer $\to$ hit | INC-0 exit |
| **INC-2** | PRODUCT | HEADLESS | Multi-output, hotplug, 1:1 camera binding, zero-output state | INC-1 exit |
| **INC-3** | PRODUCT | HEADLESS | Physical input intake (`InputPort`), `F-HIT-1` pointer routing | INC-2 exit |
| **INC-4** | PRODUCT | HEADLESS | Surface tree changes, subsurfaces, popups, frame clock | INC-3 exit |
| **INC-5** | PRODUCT | HEADLESS | Pointer constraints, `F-CONFINE-1`, Smithay P1 (`OB-11` gate) | INC-4 exit |
| **INC-6** | PRODUCT | HEADLESS | Fractional scale ladder, $H_{\text{dev}}$ device ceiling, epochs | INC-2 & INC-3 exits |
| **INC-7** | PRODUCT | HEADLESS | Window movement, placement, drag (`F-DRAG-1`) | INC-4 exit |
| **INC-8** | PRODUCT | HEADLESS | Protocol fullscreen (`F-PRESENT-1`), presentation views | INC-4 exit |
| **INC-9** | PRODUCT | HEADLESS | Focus model, activation tokens, attention requests | INC-3 & INC-4 exits |
| **INC-10** | PRODUCT | HEADLESS | Zoomed pan navigation (`F-PAN-1`), lattice quantization | INC-3 exit (`OB-24` gate) |
| **INC-11** | PRODUCT | HEADLESS | XWayland satellite integration (`D-C6`) | INC-4 exit |
| **INC-12** | PRODUCT | HEADLESS | Persistence service (`OBJ-33`), passive lookup | INC-2 exit (`G-OB48` gate) |
| **INC-13** | PRODUCT | **NESTED** | Real GLES rendering, real $D_{\text{max}}$, rotated GLES output | INC-2 & INC-6 exits |
| **INC-14** | PRODUCT | HEADLESS | Cross-output window drag (`OB-34` gate) | INC-7 exit |
| **INC-15** | PHYSICAL | HEADLESS / NESTED | Toolkit compatibility testing (GTK, Qt, Chromium) | Separate user auth |
| **INC-16** | PHYSICAL | **EXCLUSIVE_HW** | DRM/KMS, VRR, libinput, physical hotplug | Phase G / Separate auth |
| **INC-17** | PHYSICAL | **EXCLUSIVE_HW** | Long-duration stability, gaming, frame timing | INC-16 / Separate auth |

---

## 2. Deep Audit of Critical Increments

### 2.1 INC-0: Pure Enforcement Skeleton
- **Attacked Risk:** Product code or domain logic leaking into the initial enforcement commit.
- **Audit Findings:**
  - `INC-0` introduces **zero** authoritative state (`Authoritative state introduced: none`).
  - Touches no Wayland seams.
  - Satisfies `OB-19` (unconditional Phase F entry gate) by porting the authority spike's `verify.sh` negative compile probes into a dedicated `verify-architecture` crate and configuring automated CI enforcement (`OB-36`).
  - Viability: **PASS**. Can be implemented immediately upon Phase F authorization without ambiguity.

### 2.2 INC-1: Minimal Vertical Slice
- **Attacked Risk:** Premature dependency on multi-output coordination, input routing, or scale policies.
- **Audit Findings:**
  - Deliberately constrained to: 1 element (root + subsurfaces + 1 popup), 1 output, 1 camera at zoom 1.0, and `DummyRenderer`.
  - Exercises only static public-API snapshot extraction, coordinate origin computation (`F-ORIGIN-1`), lattice geometry (`F-LATTICE-1`), and physical-pixel hit selection (`F-HIT-1`).
  - Hotplug, input routing, and scale policy are explicitly excluded.
  - Viability: **PASS**. Completely implementable without any unresolved architecture choices.

### 2.3 INC-5: Pointer Constraints & P1 Boundary
- **Attacked Risk:** Framework defect P1 blocking Phase F entry or early slices.
- **Audit Findings:**
  - Pointer constraints are strictly quarantined to `INC-5`.
  - `OB-11` serves as an explicit prerequisite decision gate before shipping constraint behavior.
  - If upstream Smithay fixes the issue, stock crate is used; if not, patch P1 is applied under the `D-B12-PIN` policy.
  - Early increments (`INC-0` through `INC-4`) proceed completely unimpeded.

### 2.4 Physical Validation Discipline (INC-13, INC-15, INC-16, INC-17)
- **Attacked Risk:** Conflating headless test evidence with physical hardware validation.
- **Audit Findings:**
  - The plan enforces a strict separation: `INC-0` through `INC-12` are 100% `HEADLESS`.
  - `INC-13` introduces `NESTED` execution (requiring explicit per-run user authorization).
  - `INC-16` and `INC-17` are `EXCLUSIVE_HARDWARE` and deferred to Phase G.
  - No physical claims are falsely attributed to headless test runs.
  - `OB-46` (rotated output physical validation) and `OB-48` (cross-session persistence identity) remain open gates.
