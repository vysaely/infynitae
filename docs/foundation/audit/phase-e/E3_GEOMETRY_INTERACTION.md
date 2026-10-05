# Checkpoint E3 — Geometry, Interaction & Numeric Audit

**Reviewer:** Independent Principal Systems Architect  
**Baseline:** Phase D Final Seal (`c0a1cee310e56b75d32cf30152a09975ff776700`)

---

## 1. Coordinate Mathematics & Re-Derivation of `F-*`

The canonical coordinate formulas defined in `FOUNDATION_ARCHITECTURE.md §F` were re-derived across fixed-point and floating-point spaces:

### 1.1 Fixed-Point Canvas Space (`INV-COORD-1`, `F-ROUND-1`)
- Canvas positions are stored as Q16 fixed-point signed integers (`i64`, 2⁻¹⁶ pixels per unit).
- Maximum coordinate magnitude is bounded by $|v| \le 2^{36}$ (INV-COORD-1).
- **Float Conversion Exactness:** Because IEEE 754 `f64` contains 53 bits of mantissa, any integer $|n| \le 2^{53}$ is represented without precision loss. For Q16 values bounded by $2^{36}$, the underlying integer satisfies $|n| \le 2^{36} \cdot 2^{16} = 2^{52} < 2^{53}$. All Canvas positions convert to `f64` with **zero representation error**.

### 1.2 View Origin Derivation (`F-VIEW-1`)
- Formula: $v_O = \text{Q16}(\text{centre} - \text{mode}_O / (2 \cdot k_O))$ where $k_O = \text{zoom}_C \cdot \text{scale}_O$.
- **Range Verification:** For a window of width $2^{31}$ fitted into an output, the view origin can reach $|v_O| \le 2^{36} + 2^{30} < 2^{37}$. Because $2^{37} \cdot 2^{16} = 2^{53}$, this value remains within the exact integer mantissa limit of `f64`.

### 1.3 Reference-Relative Lattice & Snapping (`F-LATTICE-1`, `F-RENDER-1`)
- Reference: $R_O = \lfloor v_O / 120 \rfloor \cdot 120$.
- Grid offset: $C_O = \text{round}((v_O - R_O) \cdot k_O + \delta)$ with $\delta = 2^{-20}$.
- Render edge: $\text{edge}(x) = \text{round}((x - R_O) \cdot k_O + \delta) - C_O$.
- Location handed to Smithay: $L(x) = (x - R_O) \cdot k_O + \delta - C_O$.
- **Smithay Equivalence Check:** Smithay rounds $L(x)$ to integer physical pixels. Because $C_O$ is an exact integer, $\text{round}(A - C_O) = \text{round}(A) - C_O$ in integer arithmetic. Smithay's render element placement mathematically equals `F-LATTICE-1`.
- **Known Residual (OB-24):** When $k_O \notin n/120$, changing the lattice reference $R_O$ during camera motion alters physical widths and gaps by $\pm 1$ physical pixel (measured in ED-1). This is tracked under open decision `OB-24`.

---

## 2. Hit-Testing & Render Equivalence (`F-HIT-1`, `INV-HIT-1`)

### 2.1 Equivalence Proof
Hit-testing uses the identical physical projection as rendering:
1. Pointer physical coordinate: $u = \text{transform}_O(p \cdot \text{scale}_O)$, pixel $(\lfloor u_x \rfloor, \lfloor u_y \rfloor)$.
2. Render order traversal: Front-to-back according to reverse Stage z-order, popup trees, and root tree (`INV-RENDER-2`).
3. Pixel test: A surface is eligible only if its `F-LATTICE-1` rect covers physical pixel $(\lfloor u_x \rfloor, \lfloor u_y \rfloor)$.
4. Input region test: Half-open cell test on the surface-local point $(q - \text{origin})$ clamped into $[0, w) \times [0, h)$.

### 2.2 Boundary Clamping Audit
In `F-HIT-1`:
- "Delivered coordinates: $q - \text{origin}$, clamped into $[0, w) \times [0, h)$."
- Clamping prevents sub-pixel unprojection variance from delivering negative coordinates (e.g. $-0.0001$) or overflow coordinates (e.g. $w + 0.0001$) to clients across fractional scale and zoom.
- Semantic tests in `xhit.rs` confirmed 0 mismatches against presented surfaces across 16 fractional configurations.

---

## 3. Scale Policy & Interaction Epoch Audit (`INV-SCALE-*`, `F-SCALE-*`)

### 3.1 Division by Zero on Initial Commit (Finding E-F2-06)
- **Normative Formula (`F-SCALE-2`):**
  $$H_{\max}(E) = \min(H_{\text{cfg}}, D_{\max} / s_{\max}(E))$$
  where $s_{\max}(E)$ is the largest logical side over all surfaces of $E$'s tree.
- **Vulnerability:** In state machine `M1`, at state `AwaitingInitialCommit`, a client commits without a buffer. The window geometry size is $0 \times 0$, meaning $s_{\max}(E) = 0$.
- `F-SCALE-3` explicitly recognizes that the initial hint evaluates over elements whose size is $0 \times 0$ (*"or the anchor point when its size is 0×0"*).
- When $s_{\max}(E) = 0$, evaluating $D_{\max} / 0$ triggers an unhandled **integer division by zero panic** in Rust.
- **Required Correction:** $H_{\max}(E)$ must be guarded:
  $$H_{\max}(E) = \begin{cases} H_{\text{cfg}}, & \text{if } s_{\max}(E) = 0 \\ \min(H_{\text{cfg}}, D_{\max} / s_{\max}(E)), & \text{otherwise} \end{cases}$$
- **Severity: Level B.**

### 3.2 Scale Interaction Epoch & Settlement Timeout
- Interactions covered: `CameraPan`, `CameraZoom`, `WindowMove`, `WindowResize`, `ShellStream`.
- During an epoch, hint decreases are suppressed, increases occur only at rung crossings, and client responses cannot reverse direction (`INV-SCALE-3/4`).
- **300 ms Timeout Audit:**
  - If $T_{\text{settle}} < \text{repeat\_interval}$ (e.g. 10 ms), discrete zoom steps trigger a settlement event on every wheel tick, resulting in buffer renegotiation storms.
  - If $T_{\text{settle}}$ is large (e.g. 5000 ms), single zoom-out steps delay hint reduction, producing transient blur/downscaling.
  - Correctness does not depend on the exact 300 ms value; it is safely a tuning constant (`PROV-5`, `OB-23`), provided $T_{\text{settle}} > \text{discrete\_input\_interval}$.

### 3.3 Fullscreen Observation Hazard (Finding E-F2-07)
- In `F-SCALE-1` and `INV-SCALE-1`, $\rho(E) = \max_{p} (\text{scale}_p \cdot \text{zoom}_p)$ with no fullscreen override.
- A fullscreen game running on a 1080p display ($\text{scale}=1.0, \text{zoom}=1.0$) that is simultaneously observed on a 4K display with camera zoom 2.0 has $\rho = 4.0$.
- The game receives `preferred_scale = 4.0` and allocates an 8K buffer for a 1080p presentation.
- While historically acknowledged under `CP2-Q1`, this represents an unbounded resource hazard on mixed-DPI setups. Classified as **E-F2 Level C**.

---

## 4. Output Removal & Pointer Relocation Crash (Finding E-F2-05)

- **Formula (`F-RELOCATE-1`):**
  $$a = \text{arr\_origin}(O_{\text{lost}}) + p$$
  *"choose the surviving output whose Arrangement rect is nearest to a; clamp a into that rect half-open... $p' = a - \text{arr\_origin}(O')$."*
- **Vulnerability:** When the last connected monitor is disconnected (0 outputs remain), the set of surviving outputs is **empty**.
- Calling `F-RELOCATE-1` causes an iterator unwrap panic.
- `OBJ-20` defines seat pointer state as mandatory `(interaction output, OutputLogical p)`. When 0 outputs exist, no interaction output exists.
- **Required Correction:** When all outputs are removed, `Seat` pointer state must transition to an explicit detached / suspended state `None`, and `F-RELOCATE-1` must guard against empty surviving output sets.
- **Severity: Level B.**
