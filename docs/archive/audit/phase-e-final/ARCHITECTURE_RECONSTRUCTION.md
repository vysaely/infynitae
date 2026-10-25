# Phase E Final Review — Architecture Reconstruction

**Review Date:** 2026-10-05  
**Auditor:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  

---

## 1. Effective Invariant Registry Reconstruction

The living architecture documents state through layers of amendment. Rather than accepting the document checker's output on faith, an independent parser was implemented to reconstruct the effective invariant set from the raw historical layers and cross-check them against the living invariant registry (`FOUNDATION_INVARIANTS.md`).

### 1.1 Layer Audit and Decomposition

The historical layers in `FOUNDATION_ARCHITECTURE.md` define the progressive evolution of the normative invariants:

1. **Layer C12 (Phase C Seal `615b0ed`):** 54 raw rows defining the baseline invariant set.
2. **Layer C12.R1 (Phase D 1R Seal `84ce620`):** 16 raw rows amending 13 Phase C invariants and adding 3 new invariants (`INV-COORD-5`, `INV-INPUT-10`, `INV-PRES-1`), bringing the effective set to 57.
3. **Layer C12.R2 (Phase D 2R Seal `5827926`):** 6 raw rows amending 6 invariants (`INV-CAMERA-1`, `INV-CAMERA-3`, `INV-OUTPUT-1`, `INV-SCALE-3`, `INV-SCALE-4`, `INV-SCALE-5`).
4. **Layer C12.E1R (E1R Seal `1ae1b73`):** 6 raw rows amending 6 invariants (`INV-CAMERA-3`, `INV-INPUT-5`, `INV-OUTPUT-1`, `INV-OUTPUT-2`, `INV-SCALE-2`, `INV-SCALE-5`).

### 1.2 Invariant Precedence and Effective Set

Applying the chronological precedence rule:
$$\text{Effective}(I) = \text{Layer}_{\text{latest}}(I)$$
the distribution of the 57 effective invariants across their authoritative origin layers is exactly:

| Origin Layer | Count | Invariants Active |
|---|---|---|
| **C12 @ `615b0ed`** | **39** | `INV-ANIMA-1`, `INV-ANIMA-2`, `INV-ANIMA-3`, `INV-ANIMA-4`, `INV-ANIMA-5`, `INV-CAMERA-2`, `INV-CAMERA-4`, `INV-COORD-1`, `INV-COORD-4`, `INV-COORD-6`, `INV-FAIL-1`, `INV-FAIL-2`, `INV-FRAME-1`, `INV-FRAME-2`, `INV-FRAME-3`, `INV-FRAME-4`, `INV-FRAME-5`, `INV-HIT-1`, `INV-IDLE-1`, `INV-INPUT-1`, `INV-INPUT-2`, `INV-INPUT-3`, `INV-INPUT-4`, `INV-INPUT-6`, `INV-INPUT-7`, `INV-INPUT-9`, `INV-OUTPUT-3`, `INV-RENDER-1`, `INV-RENDER-2`, `INV-RENDER-3`, `INV-RENDER-4`, `INV-SCALE-1`, `INV-STAGE-1`, `INV-STAGE-2`, `INV-STAGE-3`, `INV-STAGE-4`, `INV-STAGE-5`, `INV-STAGE-6`, `INV-TIME-1` |
| **C12.R1 @ `84ce620`** | **9** | `INV-CAMERA-5`, `INV-COORD-2`, `INV-COORD-3`, `INV-COORD-5`, `INV-FRAME-6`, `INV-INPUT-8`, `INV-INPUT-10`, `INV-PRES-1`, `INV-PROTO-1` |
| **C12.R2 @ `5827926`** | **3** | `INV-CAMERA-1`, `INV-SCALE-3`, `INV-SCALE-4` |
| **C12.E1R @ `1ae1b73`** | **6** | `INV-CAMERA-3`, `INV-INPUT-5`, `INV-OUTPUT-1`, `INV-OUTPUT-2`, `INV-SCALE-2`, `INV-SCALE-5` |
| **TOTAL** | **57** | Exactly 57 effective invariants |

### 1.3 Superseded Invariants Verification

Independent check verified that superseded versions are inactive:
- `INV-CAMERA-3` is governed by `C12.E1R` (1:1 binding, `view_O` view origin, G-MIRROR gate), superseding `C12.R2`, `C12.R1`, and `C12`.
- `INV-INPUT-5` is governed by `C12.E1R` (Smithay P1 active-only deactivation, zero-output routing), superseding `C12.R1` and `C12`.
- `INV-OUTPUT-1` is governed by `C12.E1R` (`PointerState::NoOutput`, no nearest-output on $\emptyset$), superseding `C12.R2` and `C12`.
- `INV-OUTPUT-2` is governed by `C12.E1R` (`output_reconfigured` hardware-only mutation, `phys_O` mode), superseding `C12.R1` and `C12`.
- `INV-SCALE-2` is governed by `C12.E1R` ($H_{\text{dev}} = +\infty$ on $s_{\text{max}} = 0$), superseding `C12.R1` and `C12`.
- `INV-SCALE-5` is governed by `C12.E1R` ($b(h)$ integer buffer scale bound), superseding `C12.R2`, `C12.R1`, and `C12`.

### 1.4 Bidirectional Provenance Audit

Cross-comparison between `FOUNDATION_ARCHITECTURE.md` (§C12, §C12.R1, §C12.R2, §C12.E1R) and `FOUNDATION_INVARIANTS.md` (`<!-- living:INV -->`):
1. Every layer row in `FOUNDATION_ARCHITECTURE.md` has its latest active version represented verbatim in `FOUNDATION_INVARIANTS.md`.
2. Every `Origin` tag in `FOUNDATION_INVARIANTS.md` correctly resolves to its corresponding layer.
3. No orphan invariants exist.

---

## 2. Living Formula Registry Reconstruction

Independent extraction of `<!-- living:F -->` from `FOUNDATION_ARCHITECTURE.md` verified that the formula registry contains **exactly 20 living formulas**:

| Formula ID | Name | Role & Coordinate Space | Amended In |
|---|---|---|---|
| `F-VIEW-1` | Per-output view origin | Derives Canvas view origin $v_O$ using `view_O` physical extent | C12.E1R (E-F2-01) |
| `F-LATTICE-1` | Reference-relative lattice & physical edges | Computes physical edges in `view_O` space | C12.E1R (E-F2-01) |
| `F-RENDER-1` | Render location handed to Smithay | Supplies element offsets and initializes `OutputDamageTracker(phys_O)` | C12.E1R (E-F2-01) |
| `F-MEMBER-1` | Output membership | Surface/element intersection test against `[0, view_O)` | C12.E1R (E-F2-01) |
| `F-UNPROJECT-1` | OutputLogical point to Canvas (T5) | Inverts pointer position $p \to q$ using snapped camera $c_{\text{eff}, O}$ | Phase B / C |
| `F-PROJECT-1` | Canvas to OutputLogical point (T6) | Projects Canvas position $q \to p$ using snapped camera $c_{\text{eff}, O}$ | Phase B / C |
| `F-HIT-1` | Physical-pixel hit selection | View-space physical $u = p \cdot \text{scale\_O}$ hit selection | C12.E1R (E1R-N1) |
| `F-ORIGIN-1` | Single surface-origin function | Unified integral Canvas origins for roots, subsurfaces, popups | Phase B / C |
| `F-PAN-1` | Direct-manipulation pan | Canvas displacement from physical pointer deltas | Phase D 1R |
| `F-DRAG-1` | Element drag placement | Canvas window drag displacement from pointer deltas | Phase B / C |
| `F-ROUND-1` | Rounding points | Enumerates canonical rounding points R1..R8 | Phase D 1R |
| `F-LOCK-1` | Derived Canvas lock point | Surface-local lock point lifted to Canvas | Phase D 1R |
| `F-RELOCATE-1` | Pointer relocation on output loss | Nearest surviving output relocation (domain: survivors $\ne \emptyset$) | C12.E1R (E-F2-05) |
| `F-RECONF-1` | Pointer on output reconfiguration | Half-open pointer clamp into new logical extent | C12.E1R (E-F2-03) |
| `F-CONFINE-1` | Confinement clamp | Clamps pointer within projected region and output bounds | Phase D 1R |
| `F-SCALE-1` | Scale requirement $\rho$ | Maximum over observing presentations of scale $\times$ zoom | Phase C |
| `F-SCALE-2` | Ladder and device ceiling | $H_{\text{dev}}$ device bound with $s_{\text{max}} = 0 \implies +\infty$ domain fix | C12.E1R (E-F2-06) |
| `F-SCALE-3` | Target, emission and initial hint | Preferred scale emission with $b(h)$ integer bound | C12.E1R (E1R-N3) |
| `F-FIT-1` | Viewport-fit target | One-shot calculation of center and zoom to fit element | Phase C / D |
| `F-PRESENT-1` | Protocol fullscreen presentation | View-space presentation view mapping to $(0, 0)$ | C12.E1R (E-F2-01) |

---

## 3. Obligations Audit

Extraction of `<!-- living:OB -->` from `FOUNDATION_TEST_PLAN.md` confirmed exactly **51 obligations (`OB-1` through `OB-51`)**:

1. **OB-19 is strictly unconditional:**
   - `Status: OPEN`
   - `Destination: PHASE-F-ENTRY-GATE`
   - `Blocks: Phase F: unconditional entry gate; no protocol or product behaviour may precede it. Not waivable.`
2. **Newly Added Obligations (`OB-45` … `OB-51`):**
   - All 7 new obligations added by E1R are correctly marked `Status: OPEN`.
   - `OB-45`: Destination `T1`, blocks INC-3 exit and rotated-output product claims.
   - `OB-46`: Destination `T4`, blocks release claims for rotated outputs.
   - `OB-47`: Destination `T1`, blocks INC-5 exit.
   - `OB-48`: Destination `DECISION`, blocks gate G-OB48 (cross-session persisted camera claims).
   - `OB-49`: Destination `T1`, blocks INC-2 exit.
   - `OB-50`: Destination `T1`, blocks INC-5 exit.
   - `OB-51`: Destination `T1`, blocks INC-5 exit.

---

## 4. Implementation Increments Audit

Extraction of `<!-- living:INC -->` from `FOUNDATION_IMPLEMENTATION_PLAN.md` confirmed exactly **18 increments (`INC-0` through `INC-17`)**:

1. **INC-0 is Enforcement-Only:**
   - `Category: ENFORCE`
   - `Execution class: HEADLESS`
   - `Authoritative state introduced: none`
   - Satisfies `OB-19` and `OB-36` by setting up workspace skeletons, `verify-architecture`, and CI.
2. **INC-1 is Minimal Vertical Production Slice:**
   - `Category: PRODUCT`
   - `Execution class: HEADLESS`
   - Scope: 1 element (root + subsurfaces + 1 popup), 1 output, 1 camera at zoom 1, `DummyRenderer`, hit-testing. No hotplug, no input routing, no scale policy.
   - No open architecture decisions or circular prerequisites.
