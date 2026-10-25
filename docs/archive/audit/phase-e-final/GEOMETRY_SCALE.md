# Phase E Final Review — Coordinate Geometry & Scale Mathematics Audit

**Review Date:** 2026-10-05  
**Auditor:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  

---

## 1. Complete Coordinate Pipeline Derivation

The coordinate transformations linking physical hardware, Smithay's render tree, client buffers, and pointer input were re-derived from first principles.

$$\begin{aligned}
\text{Hardware Panel (Mode)} &\implies \text{phys}_O = (\text{phys}_w, \text{phys}_h) \\
\text{Transform Size Mapping} &\implies \text{view}_O = \text{transform\_size}_O(\text{phys}_O) \\
\text{Output Logical Space} &\implies \text{logical}_O = \text{view}_O / \text{scale}_O \\
\text{Canvas Top-Left View Origin} &\implies v_O = \text{Q16}\left(\text{centre} - \frac{\text{view}_O}{2 \cdot k_O}\right), \quad k_O = \text{zoom}_C \cdot \text{scale}_O \\
\text{Physical Lattice Edges} &\implies \text{edge}(x) = \text{round}((x - R_O)\cdot k_O + \delta) - C_O \in [0, \text{view}_O) \\
\text{Render Damage Tracker} &\implies \text{OutputDamageTracker::new}(\text{phys}_O, \text{Scale}(k_O), \text{transform}_O) \\
\text{Pointer View-Space Physical} &\implies u = p \cdot \text{scale}_O \in [0, \text{view}_O) \\
\text{Physical Hit Selection} &\implies (\lfloor u_x \rfloor, \lfloor u_y \rfloor) \text{ against } \text{F-LATTICE-1 rects in } [0, \text{view}_O)
\end{aligned}$$

---

## 2. Re-Attack on Finding E-F2-01: phys_O vs view_O

### 2.1 The Conflict in Phase D

Phase D passed `mode_O` (which was interpreted as the transformed size `view_O`) into `OutputDamageTracker::new`.
In Smithay 0.7.0 `src/backend/renderer/damage/mod.rs`:
```rust
let (output_size, output_scale, output_transform) = try_into(&self.mode)?;
let output_transform = output_transform.invert();
let output_geo = Rectangle::from_size(output_transform.transform_size(output_size));
let mut frame = renderer.render(framebuffer, output_size, output_transform)?;
```
1. `renderer.render` sets `glViewport(0, 0, output_size.w, output_size.h)`. This MUST match the physical framebuffer/panel dimensions $\text{phys}_O$.
2. If `output_size` is passed as the transformed size $\text{view}_O$ (e.g. $1080 \times 1920$ for a rotated 1080p panel):
   - `glViewport` receives $1080 \times 1920$ on a $1920 \times 1080$ physical framebuffer, rendering outside the panel and clipping the display.
   - `output_geo = output_transform.transform_size(output_size)` applies a second 90° rotation, resulting in $1920 \times 1080$.
   - Elements laid out in view space ($1080 \times 1920$) are clipped against $1920 \times 1080$.

### 2.2 The E1R Correction

`F-RENDER-1` explicitly feeds $\text{phys}_O$ into `OutputDamageTracker::new`:
- `output_size` is $\text{phys}_O = (1920, 1080)$.
- `glViewport` receives the true panel size $(1920, 1080)$.
- `output_geo = output_transform.transform_size(phys_O) = view_O = (1080, 1920)`.
- Elements in view space $(1080, 1920)$ match `output_geo` exactly.

---

## 3. Re-Attack on Finding E1R-N1: F-HIT-1 Transform Removal

### 3.1 The Conflict in Phase D Hit-Testing

Phase D formulated hit-testing as:
$$u = \text{transform}_O(p \cdot \text{scale}_O)$$
This applied the output transform to the pointer position.
However:
1. $p$ is delivered in `OutputLogical` coordinates, which are *already oriented in the user's view*.
2. Multiplying $p \cdot \text{scale}_O$ yields coordinates in view-space physical pixels $[0, \text{view}_O)$.
3. Element rectangles computed by `F-LATTICE-1` live in view-space physical pixels $[0, \text{view}_O)$.
4. Applying $\text{transform}_O$ rotated the pointer into panel space, causing pointer hits to diverge from displayed surfaces for every non-Normal transform.

### 3.2 Verification with `xtransform`

The test matrix in `experiments/b8-harness/src/bin/xtransform.rs` evaluates:
- Transforms: `Normal`, `90`, `180`, `270`, `Flipped`, `Flipped90`, `Flipped180`, `Flipped270`.
- Scales: `1.0`, `1.25`, `1.5`.
- Asymmetric panel: $1920 \times 1080$.

**Execution Results:**
```
  E1R rules (tracker=physical mode, hit u=p*s): 0 failing checks/mismatches over 10 cases
  SEALED tracker rule on 90/270-class transforms: failing in 7/7 cases (negative control)
  SEALED tracker rule on Normal/180/Flipped: 0 failing checks (expected 0: no swap)
  SEALED hit rule (both readings) on non-Normal transforms: failing in 18/18
RESULT: PASS (corrected passes; sealed negative controls fail as predicted)
```
The derivation of $u = p \cdot \text{scale}_O$ is mathematically and experimentally confirmed.

---

## 4. Scale Mathematics Audit: F-SCALE-2, F-SCALE-3, and E-F2-07

### 4.1 Boundary Domain at Zero Extent (Finding E-F2-06)

In Phase D, $H_{\text{max}}(E) = \min(H_{\text{cfg}}, D_{\text{max}} / s_{\text{max}}(E))$.
When an application commits an initial surface without a buffer, $s_{\text{max}}(E) = 0$, triggering a division by zero (`ZeroDivisionError`).
In E1R (`F-SCALE-2`):
$$H_{\text{dev}}(E) = \begin{cases} D_{\text{max}} / s_{\text{max}}(E) & \text{if } s_{\text{max}}(E) > 0 \\ +\infty & \text{if } s_{\text{max}}(E) = 0 \end{cases}$$
$$H_{\text{max}}(E) = \min(H_{\text{cfg}}, H_{\text{dev}}(E))$$
For $s_{\text{max}}(E) = 0$, $H_{\text{max}}(E) = \min(H_{\text{cfg}}, +\infty) = H_{\text{cfg}}$. The domain is continuous and finite.

### 4.2 Integer Scale Emission Bound (Finding E1R-N3)

When emitting `preferred_buffer_scale`, Phase D used $\lceil h \rceil$. If $H_{\text{dev}}(E)$ is non-integer (e.g. $s_{\text{max}} = 8000$, $D_{\text{max}} = 16384 \implies H_{\text{dev}} = 2.048$), $\lceil 2.048 \rceil = 3$. An integer client would allocate $8000 \times 3 = 24000 > D_{\text{max}}$.
In E1R (`F-SCALE-3`):
$$b(h) = \begin{cases} \lceil h \rceil & \text{if } \lceil h \rceil \le H_{\text{dev}}(E) \\ \max(1, \lfloor H_{\text{dev}}(E) \rfloor) & \text{otherwise} \end{cases}$$
For $h = 2.048$, $b(h) = \max(1, 2) = 2$. Buffer allocated: $8000 \times 2 = 16000 \le 16384$. Bounded.

### 4.3 Re-Evaluation and Disproval of Finding E-F2-07

Phase E claimed that a 1080p fullscreen window observed by a 4K display at zoom 2.0 would request an 8K buffer ($7680 \times 4320$).
Mathematical re-evaluation:
1. Display A: 1080p, scale 1.0, fullscreen presentation $\implies \text{term}_A = 1.0 \times 1.0 = 1.0$.
2. Display B: 4K, scale 2.0, camera observing window at zoom 2.0 $\implies \text{term}_B = 2.0 \times 2.0 = 4.0$.
3. Requirement $\rho(E) = \max(1.0, 4.0) = 4.0$.
4. Target $t(\rho)$ in `F-SCALE-3` clamps to $H_{\text{max}}(E) = \min(H_{\text{cfg}}, H_{\text{dev}}(E))$.
5. Here $H_{\text{cfg}} = 3.0$, $H_{\text{dev}} = 16384 / 1920 = 8.533 \implies H_{\text{max}} = 3.0$.
6. Therefore, $h = 3.0$, and the buffer requested is $1920 \times 3 = 5760$ and $1080 \times 3 = 3240$.
7. $5760 \times 3240$ is **5.7K**, not 8K ($7680 \times 4320$), and comfortably fits within $D_{\text{max}} = 16384$.
8. Finding E-F2-07 remains **conclusively disproved**.
