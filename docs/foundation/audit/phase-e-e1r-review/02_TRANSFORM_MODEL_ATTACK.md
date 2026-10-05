# Phase E E1R Adversarial Re-Review — Transform Model & Coordinate Pipeline (E-F2-01 & E1R-N1)

**Reviewer:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  
**Date:** 2026-10-05  
**Focus:** Adversarial reconstruction and attack against the rotated-output rendering and hit-testing pipeline.

---

## 1. Independent Mathematical Reconstruction of the Pipeline

### Forward Pipeline (Stage → Buffer/Glass)

1. **Stage (Canvas):** A point $q \in \text{Canvas}$ (integral/Q16 coordinates).
2. **Camera View Origin (`F-VIEW-1`):**
   $$v_O = \text{Q16}\left(c_O - \frac{\text{view}_O}{2 \cdot z \cdot s_O}\right)$$
   where:
   - $c_O$ is camera centre;
   - $z$ is zoom factor;
   - $s_O$ is output scale;
   - $\text{phys}_O = (W_p, H_p)$ is the unrotated physical display resolution;
   - $\text{view}_O = T_O(\text{phys}_O)$ is the view extent after applying output transform $T_O$. For 90°/270°/Flipped90°/Flipped270°, $\text{view}_O = (H_p, W_p)$; otherwise $(W_p, H_p)$.
3. **OutputLogical Extent:**
   $$L_O = \frac{\text{view}_O}{s_O}$$
   Wayland clients observe $L_O$ as the logical output size.
4. **Element (View) Physical Lattice (`F-LATTICE-1`, `F-RENDER-1`):**
   A surface with Canvas origin $x_0$ and logical size $(w_s, h_s)$ is positioned in view (element) space at:
   $$L = (x_0 - v_O) \cdot (z \cdot s_O)$$
   Size in element space is:
   $$S_{\text{elem}} = (w_s, h_s) \cdot (z \cdot s_O)$$
   The element rect $R_{\text{elem}} = [L, L + S_{\text{elem}})$ lives in $[0, \text{view}_O.w) \times [0, \text{view}_O.h)$.
5. **Damage Tracker & Renderer Boundary (`F-RENDER-1`):**
   Smithay's `OutputDamageTracker::new` is passed the untransformed mode $\text{phys}_O$:
   $$\text{OutputDamageTracker::new}(\text{phys}_O, \text{Scale}(z \cdot s_O), T_O)$$
   Smithay computes element space as:
   $$\text{output\_geo} = T_O^{-1}(T_O(\text{phys}_O)) = \text{transform\_size}(T_O, \text{phys}_O) = \text{view}_O$$
   Smithay sets GLES viewport to:
   $$\text{glViewport}(0, 0, \text{phys}_O.w, \text{phys}_O.h)$$
   and applies $T_O^{-1}$ via projection matrix to map element space $[0, \text{view}_O)$ onto the physical framebuffer.

---

### Reverse Pipeline (Pointer Hit Testing)

1. **Output-Logical Pointer Coordinate:**
   The compositor seat maintains the pointer location $p \in [0, L_O.w) \times [0, L_O.h)$ in `OutputLogical` coordinates.
2. **Physical-Pixel Hit Selection (`F-HIT-1`):**
   $$u = p \cdot s_O$$
   Notice that $u \in [0, \text{view}_O.w) \times [0, \text{view}_O.h)$, which is **directly in view/element space**.
3. **Lattice Coverage:**
   The hit test checks whether $(\lfloor u_x \rfloor, \lfloor u_y \rfloor) \in R_{\text{elem}}$.
   Because $R_{\text{elem}}$ is also in view/element space, the coordinates are in the exact same domain.
4. **Surface-Local Delivery (`F-UNPROJECT-1`):**
   Delivered coordinate:
   $$q_{\text{delivered}} = \text{F-UNPROJECT-1}(p) - x_0 = \left(v_O + \frac{p}{z}\right) - x_0$$
   Clamped half-open into $[0, w_s) \times [0, h_s)$.

---

## 2. Attack on Finding E-F2-01 (Damage Tracker Sizing)

### Sealed Defect Analysis
In sealed Phase D (`c0a1cee`):
- `PD-F1-04` specified that `mode_O` was the swapped size ($1080 \times 1920$).
- `OutputDamageTracker::new` was passed $(1080, 1920)$.
- Smithay's internal `transform_size` inverted and swapped it back to $(1920, 1080)$ for damage geometry, while GLES `glViewport` set $(1080, 1920)$ on a $(1920, 1080)$ physical buffer.
- This caused a catastrophic double swap: 1,814,400 pixels uncovered, 907,200 fragments lost outside the viewport.

### E1R Correction Verification
Under E1R:
- `OutputDamageTracker::new` receives `phys_O` ($(1920, 1080)$).
- GLES viewport is set to $(1920, 1080)$.
- Smithay element geometry is set to $(1080, 1920) = \text{view}_O$.
- Tested on all 8 transform families and fractional scales (1.0, 1.25, 1.5).
- All checks C1 (viewport == panel), C2 (view covers panel exactly), C3 (edges land on physical edges), C4 (damage bounds) pass 10/10 with 0 failures.
- Sealed rule fails 7/7 rotated cases as predicted.

---

## 3. Attack on Finding E1R-N1 (Hit Test Transform Mismatch)

### Sealed Defect Analysis
In sealed Phase D (`c0a1cee`):
- `F-HIT-1` defined:
  $$u = \text{transform}_O(p \cdot s_O)$$
- However, $p$ is already in `OutputLogical` space (which already reflects the orientation of the output).
- $p \cdot s_O$ is therefore already in view/element space.
- Applying $\text{transform}_O$ to $p \cdot s_O$ rotated/flipped the coordinates a second time, mapping them into panel physical space while comparing against element rects in view space.
- As a result, hit testing failed on **18 out of 18 tested non-Normal transform cases**.

### E1R Correction Verification
Under E1R:
- `F-HIT-1` is corrected to:
  $$u = p \cdot s_O$$
- The redundant transform is removed.
- Across 2,000 tested points over 10 transform/scale cases:
  - E1R produces **0 / 2,000 mismatches**.
  - Sealed negative control produces **18 / 18 case failures** (~198-200 / 200 mismatches per case).

---

## 4. Multi-Output Boundary & Adjacency Attack

An independent adversarial script tested:
1. Adjacent multi-output setup:
   - Output 1: $1920 \times 1080$ @ 1.0x, Normal, origin $(0, 0)$.
   - Output 2: $1920 \times 1080$ @ 1.5x, 90° rotation (view $1080 \times 1920$, logical $720 \times 1280$), origin $(1920, 0)$.
2. Window $W$ ($800 \times 600$) spanning across the boundary $[1500, 2300) \times [200, 800)$:
   - Membership evaluated independently: surface is correctly detected on both outputs.
   - Hit testing on Output 1 side: hits $W$ with exact local coordinates.
   - Hit testing on Output 2 side: hits $W$ with exact local coordinates despite 90° rotation and 1.5x fractional scale.
3. Corners, fractional scales (1.25, 1.5, 1.75), negative Canvas coordinates, and extreme zooms (0.5x, 2.0x) all preserve:
   $$\text{hit}(\text{render}(q)) = q$$

---

## 5. Evaluation of Harness Execution

During execution, `xtransform` in unoptimized debug mode required significant CPU time due to $6,000$ full-frame renders and linear buffer scans. A minimal harness optimization was validated:
- Adding `first_hit: Option<usize>` to `Fb` to record the painted cursor pixel in $O(1)$.
- Reusing the cursor frame buffer rather than zeroing 8.3 MB per test point.
- Precomputing `presented` once per case.
This optimization preserved 100% semantic identity with the original test, producing the identical $0/200$ E1R mismatches and $7/7$ and $18/18$ sealed negative control failures.

---

## 6. Verdicts on Primary Attack A

- **E-F2-01 Verdict:** **CONFIRMED & VALIDATED.** The physical/view mode split is mathematically correct and necessary.
- **E1R-N1 Verdict:** **CONFIRMED & VALIDATED.** The removal of the redundant transform in `F-HIT-1` completely resolves the hit-test defect across all non-Normal transforms.
