# Phase E — Adversarial Architecture Review Findings

**Reviewer:** Independent Principal Systems Architect  
**Baseline:** Phase D Final Seal (`c0a1cee310e56b75d32cf30152a09975ff776700`)

---

## Summary of Findings by Class & Severity

| Classification | Count | Severity / Subcategory | Blocks Phase F |
|---|---|---|---|
| **E-F2 (Architecture Finding)** | **7** | Level A: 2<br>Level B: 4<br>Level C: 1 | **YES (2 Level A findings)** |
| **E-F1 (Non-semantic Refinement)** | **1** | — | NO |
| **E-F3 (Validation Gap)** | **3** | — | NO |
| **E-F0 (Editorial / Bookkeeping)** | **0** | — | NO |
| **Total Findings** | **11** | — | **Phase F Entry Gate Blocked** |

---

## Detailed Findings Catalogue

### Finding E-F2-01
- **ID:** `E-F2-01`
- **Class:** E-F2
- **Severity:** Level A
- **Claim attacked:** `F-VIEW-1`, `F-RENDER-1`, `PD-F1-04`, `INV-RENDER-1`
- **Repository evidence:**
  - `FOUNDATION_ARCHITECTURE.md:1814` (`PD-F1-04` definition: *"mode_O is the output mode after the output transform (rotated outputs swap width and height)"*)
  - `FOUNDATION_ARCHITECTURE.md:1831` (`OutputDamageTracker::new(mode_O, Scale(k_O), transform_O)`)
  - Smithay 0.7.0 `src/backend/renderer/damage/mod.rs:330-340`
  - Smithay 0.7.0 `src/backend/renderer/gles/mod.rs:2056-2068`
- **Counterexample / attack sequence:**
  1. A display with physical mode $1920 \times 1080$ is rotated $90^\circ$ (`Transform::_90`).
  2. Per `PD-F1-04`, `mode_O` is defined as swapped: $1080 \times 1920$.
  3. `F-RENDER-1` constructs Smithay's damage tracker with `OutputDamageTracker::new(mode_O, Scale(k_O), transform_O)`, passing $(1080, 1920)$.
  4. In `damage_tracker.render_output()`, Smithay reads `output_size = (1080, 1920)` and calls `output_transform.invert().transform_size(output_size)`. This swaps $(1080, 1920)$ back to $(1920, 1080)$ for damage calculations.
  5. In GLES renderer `render()`, Smithay receives `output_size = (1080, 1920)` and executes:
     ```rust
     self.gl.Viewport(0, 0, output_size.w, output_size.h); // glViewport(0, 0, 1080, 1920)
     if let Transform::_90 = transform {
         mem::swap(&mut output_size.w, &mut output_size.h); // output_size becomes 1920x1080
     }
     let x = 2.0 / (output_size.w as f32); // projection matrix uses 1920x1080
     ```
  6. The physical DRM/EGL surface is $1920 \times 1080$. `glViewport` is set to $1080 \times 1920$, clipping the horizontal rendering to 1080 pixels and specifying a vertical viewport exceeding the framebuffer height by 840 pixels. The projection matrix then swaps dimensions again. Rendering on rotated outputs is completely distorted and clipped.
- **Why existing evidence fails to cover it:** Every single experiment in `experiments/` hardcoded `Transform::Normal`. Not a single test in the repository ever tested rotated displays.
- **Impact:** Any rotated monitor produces corrupted, clipped rendering and broken damage regions on GLES/DRM hardware.
- **Required correction or validation:** Separate logical view mode from physical hardware mode. `mode_O` passed to `OutputDamageTracker::new` must be the untransformed physical mode ($1920 \times 1080$), while view derivation (`F-VIEW-1`) uses the transformed viewport ($1080 \times 1920$).
- **Blocks Phase F:** **YES**

---

### Finding E-F2-02
- **ID:** `E-F2-02`
- **Class:** E-F2
- **Severity:** Level A
- **Claim attacked:** `INV-INPUT-5`, `M13`, `W-SM-4a`, `OBJ-21`
- **Repository evidence:**
  - `FOUNDATION_INVARIANTS.md:385` (*"An explicit deactivate is used only when the delivered focus does not change in that batch; otherwise Smithay's leave() does it (W-SM-4a)"*)
  - Smithay 0.7.0 `src/wayland/pointer_constraints.rs:143-158` (`deactivate`)
  - Smithay 0.7.0 `src/wayland/seat/pointer.rs:251-255` (`leave`)
- **Counterexample / attack sequence:**
  1. A client creates a `Persistent` pointer lock (`zwp_pointer_constraints_v1.lock_pointer(lifetime=Persistent)`). The lock is active.
  2. The user pans the camera so that the lock point is panned off-screen.
  3. Under `INV-INPUT-3`, pointer focus is retained on the surface (`KeepFocus`).
  4. Pan finishes. Batch-end reconciliation runs. The lock point is no longer observable, but delivered pointer focus has not changed.
  5. Per `INV-INPUT-5`, Infynitae invokes `constraint.deactivate()` explicitly.
  6. Smithay's `deactivate()` executes: `locked.handle.unlocked()` is sent on the wire. For `Persistent` constraints, `remove_entry()` is **not** called; the constraint remains registered in `data.constraints`.
  7. In a later batch, the user clicks another window or navigates away. Smithay calls `pointer.leave()` on the original surface.
  8. In `pointer.leave()`, Smithay unconditionally executes:
     ```rust
     with_pointer_constraint(self, &pointer, |constraint| {
         if let Some(constraint) = constraint { constraint.deactivate(); }
     });
     ```
  9. `with_pointer_constraint` finds the still-registered persistent constraint and calls `deactivate()` again.
  10. `locked.handle.unlocked()` is sent to the client a **second time**, with no intervening `locked` event.
- **Why existing evidence fails to cover it:** `xlock_grab.rs` tested relocation during an implicit mouse button grab, where focus was cleared in the exact same batch. It never tested subsequent focus exit after an explicit deactivation in a prior batch.
- **Impact:** Duplicate `unlocked` events violate Wayland protocol state machine semantics, triggering fatal client asserts in SDL3, Chromium, and game runtimes.
- **Required correction or validation:** Persistent constraints explicitly deactivated while retaining focus must be unlinked from Smithay's `data.constraints` or patched in Smithay under `OB-11` to prevent `leave()` from re-emitting `unlocked`.
- **Blocks Phase F:** **YES**

---

### Finding E-F2-03
- **ID:** `E-F2-03`
- **Class:** E-F2
- **Severity:** Level B
- **Claim attacked:** `INV-OUTPUT-2`, `M4` (row 2), `OBJ-10`
- **Repository evidence:**
  - `FOUNDATION_INVARIANTS.md:805` (`INV-OUTPUT-2` defines only `output_connected` and `output_disconnected`)
  - `FOUNDATION_STATE_MACHINES.md:63` (`M4` row 2 specifies *"mode/scale/transform change (descriptor update through the token entry)"*)
  - `experiments/c1-authority-spike/core/src/lib.rs:359-380`
- **Counterexample / attack sequence:**
  1. A connected monitor changes its resolution (e.g. 4K mode switch to 1440p) or fractional scale at runtime.
  2. The backend attempts to notify `Core` via `OutputLifecycleToken`.
  3. `INV-OUTPUT-2` provides only `output_connected` and `output_disconnected`.
  4. Calling `output_connected` allocates a brand new `OutputId` (`next_output += 1`) and leaves the old `OutputId` orphaned in `self.outputs`.
  5. Calling `output_disconnected` followed by `output_connected` tears down the output: fullscreen presentations are aborted, constraints are broken, pointer is relocated, and clients receive `wl_surface.leave`.
- **Why existing evidence fails to cover it:** The authority spike only tested initial connection and final disconnect; dynamic mode changes were never called in the spike.
- **Impact:** Runtime display resolution, scale, or refresh rate changes cannot be processed without tearing down output state.
- **Required correction or validation:** Add a dedicated entry point to `Core`:
  `output_reconfigured(&mut OutputLifecycleToken, OutputId, OutputDescriptor)` implementing `M4` row 2.
- **Blocks Phase F:** NO (Can be implemented prior to INC-2 multi-output work).

---

### Finding E-F2-04
- **ID:** `E-F2-04`
- **Class:** E-F2
- **Severity:** Level B
- **Claim attacked:** `INV-CAMERA-3`, `INV-OUTPUT-2`, `OBJ-07`
- **Repository evidence:**
  - `FOUNDATION_INVARIANTS.md:133` (*"N outputs may share one camera"*)
  - `experiments/c1-authority-spike/core/src/lib.rs:69` (`bound: Vec<Option<u64>>`)
- **Counterexample / attack sequence:**
  1. An operator connects two physical monitors to separate connectors (`DP-1`, `DP-2`), intending them to mirror the same camera view.
  2. Backend calls `output_connected` for `DP-1` and `DP-2`.
  3. Because connector IDs differ, `OutputKey` differs. `output_connected` creates two independent cameras.
  4. Neither `Core`, `ShellPort`, nor `OutputDescriptor` provides an API or command to bind `DP-2` to `DP-1`'s camera.
  5. Furthermore, `CameraSet.bound` in the authority spike is `Vec<Option<u64>>`, mathematically restricting each camera to at most one output.
- **Why existing evidence fails to cover it:** The multi-output spike tested outputs with independent cameras only; shared camera binding was never exercised.
- **Impact:** Normative invariant `INV-CAMERA-3` is impossible to satisfy under the sealed 2R authority API.
- **Required correction or validation:** Either amend `INV-CAMERA-3` to explicitly restrict Foundation 01 to 1:1 camera-to-output bindings, or define an authorized shell command / configuration mechanism with supporting data structures (`bound: Vec<Vec<u64>>`) to bind multiple outputs to one camera.
- **Blocks Phase F:** NO (Can be resolved prior to INC-2).

---

### Finding E-F2-05
- **ID:** `E-F2-05`
- **Class:** E-F2
- **Severity:** Level B
- **Claim attacked:** `F-RELOCATE-1`, `OBJ-20`, `INV-OUTPUT-1`, `M4`
- **Repository evidence:**
  - `FOUNDATION_ARCHITECTURE.md:1878` (`F-RELOCATE-1`: *"choose the surviving output whose Arrangement rect is nearest to a"*)
  - `FOUNDATION_OBJECT_MODEL.md:202` (`OBJ-20`: mandatory `(interaction output, OutputLogical p)`)
- **Counterexample / attack sequence:**
  1. A single monitor is connected.
  2. The user unplugs the monitor (e.g. laptop undocked or screen turned off).
  3. `output_disconnected` is called.
  4. `INV-OUTPUT-1` invokes `F-RELOCATE-1`.
  5. The set of surviving outputs is empty ($\emptyset$).
  6. `F-RELOCATE-1` attempts to select the nearest surviving output from an empty iterator and panics.
  7. `OBJ-20` cannot represent the pointer state because `interaction output` has no valid ID.
- **Why existing evidence fails to cover it:** Harness tests never removed all outputs simultaneously; at least one output remained active.
- **Impact:** Compositor panics and crashes whenever the last display is disconnected.
- **Required correction or validation:** Update `OBJ-20` to `Option<(interaction output, OutputLogical p)>` and update `F-RELOCATE-1` to suspend pointer relocation when the surviving output set is empty, restoring historical Phase C policy (ARCHITECTURE:395).
- **Blocks Phase F:** NO (Can be resolved prior to INC-2/INC-5).

---

### Finding E-F2-06
- **ID:** `E-F2-06`
- **Class:** E-F2
- **Severity:** Level B
- **Claim attacked:** `F-SCALE-2`, `INV-SCALE-2`, `M1`
- **Repository evidence:**
  - `FOUNDATION_ARCHITECTURE.md:1891` (`F-SCALE-2`: $H_{\max}(E) = \min(H_{\text{cfg}}, D_{\max} / s_{\max}(E))$)
  - `FOUNDATION_STATE_MACHINES.md:23` (`M1`: `commit without buffer` -> `initial hint t(ρ_pred)`)
- **Counterexample / attack sequence:**
  1. A client creates a new toplevel and commits without a buffer (`AwaitingInitialCommit`).
  2. `M1` executes placement and evaluates the initial scale hint `t(ρ_pred)`.
  3. At this point, the surface has committed no buffer and has window geometry size $0 \times 0$.
  4. $s_{\max}(E) = 0$.
  5. `F-SCALE-2` evaluates $D_{\max} / s_{\max}(E) = 16384 / 0$.
  6. Integer division by zero triggers an unhandled panic in Rust.
- **Why existing evidence fails to cover it:** `cp3a-scale-model/model.py` hardcoded non-zero window dimensions `(800, 600)` in its test harness.
- **Impact:** Compositor panics on initial client commit before initial configure.
- **Required correction or validation:** Add an explicit guard in `F-SCALE-2`:
  $$H_{\max}(E) = \begin{cases} H_{\text{cfg}}, & \text{if } s_{\max}(E) = 0 \\ \min(H_{\text{cfg}}, D_{\max} / s_{\max}(E)), & \text{otherwise} \end{cases}$$
- **Blocks Phase F:** NO (Can be resolved prior to INC-6).

---

### Finding E-F2-07
- **ID:** `E-F2-07`
- **Class:** E-F2
- **Severity:** Level C
- **Claim attacked:** `INV-SCALE-1`, `F-SCALE-1`, `C5.2`
- **Repository evidence:**
  - `FOUNDATION_INVARIANTS.md:455` (`INV-SCALE-1`: *"no fullscreen override"*)
  - `FOUNDATION_ARCHITECTURE.md:612` (*"A fullscreen game on A that is also visible on B renders at 1.5x (2880x1620 buffer)"*)
- **Counterexample / attack sequence:**
  1. A client runs in protocol fullscreen on Output A ($1920 \times 1080$, scale 1.0).
  2. Output B is a 4K display (scale 2.0). The user's camera on Output B observes the region of Canvas where the client window is anchored at zoom 2.0.
  3. Under `INV-SCALE-1`, $\rho(E) = \max(1.0 \times 1.0, 2.0 \times 2.0) = 4.0$.
  4. The client is sent `preferred_scale = 4.0` and allocates an $7680 \times 4320$ (8K) buffer for a 1080p display presentation.
- **Why existing evidence fails to cover it:** Acknowledged in Phase C under `CP2-Q1`, but frozen without mitigation.
- **Impact:** Severe GPU memory and fill-rate exhaustion for fullscreen games in multi-output environments.
- **Required correction or validation:** Implement `CP2-Q1` proposal: while an element is in exclusive fullscreen presentation on output $O$, its scale requirement $\rho(E)$ should be clamped to $\text{scale}_O$.
- **Blocks Phase F:** NO.

---

### Finding E-F1-01
- **ID:** `E-F1-01`
- **Class:** E-F1
- **Severity:** N/A
- **Claim attacked:** `FOUNDATION_DECISIONS.md`, `tools/check-foundation-docs`
- **Repository evidence:**
  - `FOUNDATION_DECISIONS.md:51,78,109,...` (`- **Invariants:** STAGE-1, STAGE-2`)
  - `tools/check-foundation-docs:37` (`REF['INV'] = r'\bINV-[A-Z]+-\d+\b'`)
- **Counterexample / attack sequence:**
  In `FOUNDATION_DECISIONS.md`, all ADRs cite invariants without the `INV-` prefix. Because the checker's regex strictly requires `INV-`, none of these references are validated. An invalid token (e.g. `NONEXISTENT-999`) passes checker execution undetected.
- **Why existing evidence fails to cover it:** The checker self-tests only verified tokens matching `INV-*`.
- **Impact:** ADR invariant citations are entirely unverified by CI.
- **Required correction or validation:** Update ADR invariant citations to use canonical `INV-*` tokens, or update `tools/check-foundation-docs` to match bare invariant prefixes.
- **Blocks Phase F:** NO.

---

### Finding E-F3-01
- **ID:** `E-F3-01`
- **Class:** E-F3
- **Severity:** N/A
- **Claim attacked:** `tools/check-foundation-docs`, `FOUNDATION_ARCHITECTURE.md §F`
- **Repository evidence:**
  - `tools/check-foundation-docs:166-190`
- **Counterexample / attack sequence:**
  The checker validates formulas solely by extracting the text fragment in `- **Extracted invariant text:**` and substituting it into `FOUNDATION_INVARIANTS.md`. It never parses or verifies the lines `- **Definition:** ...`. A corrupted or contradictory formula in `- **Definition:**` passes validation without error.
- **Why existing evidence fails to cover it:** Self-test fixtures only mutated the extracted fragment line.
- **Impact:** False confidence in formula integrity.
- **Required correction or validation:** Enhance the checker to perform basic syntax and symbol verification on `- **Definition:**` lines.
- **Blocks Phase F:** NO.

---

### Finding E-F3-02
- **ID:** `E-F3-02`
- **Class:** E-F3
- **Severity:** N/A
- **Claim attacked:** `experiments/b8-harness` test coverage
- **Repository evidence:**
  - `experiments/b8-harness/src/server.rs:225,421` (All tests use `Transform::Normal`)
- **Counterexample / attack sequence:**
  Grep verification across `experiments/` reveals that not a single test in the repository ever executed an output with `Transform::_90`, `_180`, or `_270`.
- **Why existing evidence fails to cover it:** Complete absence of non-normal output transform tests in the test harness.
- **Impact:** Enabled Finding `E-F2-01` to remain undetected throughout Phase B, C, and D.
- **Required correction or validation:** Add transformed output test matrices to `b8-harness` and `INC-2` exit tests.
- **Blocks Phase F:** NO.

---

### Finding E-F3-03
- **ID:** `E-F3-03`
- **Class:** E-F3
- **Severity:** N/A
- **Claim attacked:** `OBJ-07`, `OutputKey`, `INV-OUTPUT-2`
- **Repository evidence:**
  - `experiments/c1-authority-spike/core/src/lib.rs:57-60`
- **Counterexample / attack sequence:**
  When multiple monitors of the same model lacking unique serial numbers are connected to the same port sequentially, or in headless/virtual environments where EDID serial is 0, `OutputKey` matches identically, causing camera state reuse across distinct physical monitors.
- **Why existing evidence fails to cover it:** Headless tests assigned arbitrary distinct integers to `OutputKey`.
- **Impact:** Unexpected view retention when swapping serial-less monitors on the same connector.
- **Required correction or validation:** Document as an accepted platform limitation or add connector-specific generation counters.
- **Blocks Phase F:** NO.
