# Phase E E1R Adversarial Re-Review — Secondary Findings & Claims Audit

**Reviewer:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  
**Date:** 2026-10-05  

---

## 1. Attack on E-F2-03 (Output Reconfiguration Authority)

### Attack Vector
Attempt to misuse `output_reconfigured` to bypass authority boundaries:
1. **Forge capability:** Attempt to invoke `output_reconfigured` without `OutputLifecycleToken` or with forged tokens.
   - Result: Blocked by compiler. `OutputLifecycleToken` fields are private and minted exclusively in `Core::new` (`verify.sh` probes p10, p11).
2. **Coordinate / Camera injection:** Attempt to pass camera centre or zoom into `OutputDescriptor`.
   - Result: Blocked by compiler. `OutputDescriptor` contains hardware fields only (`verify.sh` probes p12, p13).
3. **Camera allocation / hijacking:** Attempt to rebind or allocate new cameras during reconfiguration.
   - Result: `output_reconfigured` keeps the same `OutputId` and retains the existing bound camera (`test output_reconfigured_keeps_identity_and_camera`).
4. **Identity change:** Attempt to change `OutputKey` via reconfiguration.
   - Result: Explicitly rejected with `Err(AuthorityError::IdentityChanged)`. An identity change requires disconnect + connect.
5. **State coherence:** Reconfiguration triggers `ScaleInteractionEpoch` settlement and `INV-INPUT-5` constraint reconciliation. Pointers outside the new logical extent are clamped half-open via `F-RECONF-1`.

**Verdict:** **CONFIRMED & VALIDATED (PASS).**

---

## 2. Attack on E-F2-04 (1:1 Camera Cardinality)

### Attack Vector
Search for surviving claims of "N outputs may share one camera":
- Audited `FOUNDATION_INVARIANTS.md`, `FOUNDATION_OBJECT_MODEL.md`, `FOUNDATION_STATE_MACHINES.md`, `FOUNDATION_DECISIONS.md`, `FOUNDATION_IMPLEMENTATION_PLAN.md`.
- Living `INV-CAMERA-3` now explicitly mandates:
  > *"Binding is 1:1 in Foundation 01: each connected output is bound to exactly one camera, and a camera is bound to at most one output. Shared cameras (mirroring) are not part of Foundation 01 (gate G-MIRROR)."*
- Living `OBJ-07` (`CameraSet.bound: Vec<Option<u64>>`) strictly models 1:1 mapping.
- Living `M5` row 3 enforces that duplicate keys never re-bind an already-bound camera.
- All historical occurrences of "N outputs" remain strictly in frozen historical sections (`C12`, `C12.R2`, historical ledgers).
- Gate `G-MIRROR` in `FOUNDATION_IMPLEMENTATION_PLAN.md` strictly blocks multi-output shared camera mirroring from entering Foundation 01.

**Verdict:** **CONFIRMED & VALIDATED (PASS).**

---

## 3. Attack on E-F2-05 (Zero Outputs Lifecycle)

### Attack Vector
Test total display disconnection (all monitors disconnected):
1. **Empty survivor set:** `F-RELOCATE-1` previously attempted to find the nearest surviving output in an empty set $\emptyset$, triggering an unhandled panic (reproduced in `xzero.rs` negative control N1).
2. **Implicit client grab:** If a client holds a mouse button when the last output vanishes, stock Smithay's implicit grab retains focus on the vanished surface (reproduced in N2).
3. **Unset grab absolute motion leak:** Restoring focus with public `unset_grab` emits an absolute `wl_pointer.motion` to a locked surface, violating `INV-INPUT-5` (reproduced in N3).
4. **E1R Correction:**
   - On total disconnect, `F-RELOCATE-1` domain rule checks $S \ne \emptyset$. If $S = \emptyset$, seat enters `PointerState::NoOutput`.
   - Compositor installs an internal zero-output grab with `Focus::Clear`, cleanly emitting `wl_pointer.leave` and exactly-once `unlocked`/`unconfined`.
   - Physical pointer inputs are consumed and discarded.
   - `ActiveElementFocus` and keyboard focus are preserved (not output-bound).
   - On reconnect, zero-output grab is dismissed, and pointer is placed at the centre of the new output.
   - Executable verification in `xzero.rs` passes all 6 lifecycle scenarios.

**Verdict:** **CONFIRMED & VALIDATED (PASS).**

---

## 4. Attack on E-F2-06 & E1R-N3 (Scale Domain & Device Ceiling)

### Attack Vector
1. **Zero extent singularity (E-F2-06):**
   - At initial commit before buffer attachment, window extent is $0 \times 0$ ($s_{\max} = 0$).
   - Previous formula $H_{\max}(E) = \min(H_{\text{cfg}}, D_{\max} / s_{\max}(E))$ resulted in division by zero.
   - E1R corrects domain via $H_{\text{dev}}(E) = \sup\{h > 0 : h \cdot \text{side} \le D_{\max}\} = +\infty$ when $s_{\max} = 0$.
   - $H_{\max}(E) = \min(H_{\text{cfg}}, +\infty) = H_{\text{cfg}} = 3.0$. Finite and mathematically well-defined.
2. **Integer buffer scale overflow (E1R-N3):**
   - Sealed formula emitted $\lceil h \rceil$ for integer scale.
   - For an 8,000 px window side on an output where $H_{\text{dev}} = 2.048$ ($D_{\max} = 16384$), $h = 2.048$.
   - $\lceil 2.048 \rceil = 3$, requesting $8000 \times 3 = 24,000$ px buffer, exceeding $D_{\max}$ by 7,616 px.
   - E1R corrects emission rule to $b(h) = \lceil h \rceil$ if $\lceil h \rceil \le H_{\text{dev}}$, else $\max(1, \lfloor H_{\text{dev}} \rfloor)$.
   - Tested across all edge cases ($0 \times 0$, $0 \times N$, $N \times 0$, huge $20000 \times 20000$, boundaries): zero division, overflow, and bound violations are completely eliminated.

**Verdict:** **CONFIRMED & VALIDATED (PASS).**

---

## 5. Recheck of E-F2-07 Disproval (Fullscreen Scale)

### Re-Review Analysis
Phase E claimed that a 1080p fullscreen game observed by a 4K display at zoom 2.0 would request an 8K buffer ($7680 \times 4320$).
1. Canonical formulas enforce:
   $$h \le H_{\max}(E) = \min(H_{\text{cfg}}, D_{\max} / s_{\max}) \le H_{\text{cfg}} = 3.0$$
2. Desired scale $\rho = 4.0$ is clamped to target $h = 3.0$.
3. Preferred buffer size is $1920 \times 3 \times 1080 \times 3 = 5760 \times 3240$ (74.6 MB), which is well below $D_{\max} = 16384$ and is NOT 8K.
4. The $\times 9$ pixel cost is the explicit, accepted trade-off under Risk `RB-7` to ensure spatial canvas zoom fidelity.
5. The reviewer's proposed clamp ($\rho := \text{scale}_O$) is an open product policy question (`CP2-Q1`), not a correctness bug.

**Verdict:** **DISPROVAL UPHELD (PASS).**

---

## 6. Attack on E-F3-03 (Output Persistence Identity)

### Attack Vector
1. When two identical monitors lacking unique EDID serials connect simultaneously to the same system:
   - Sealed rule: Second output re-bound camera from first output, leaving first output orphaned with `None`.
   - E1R rule (`INV-CAMERA-3`): Re-binds only the lowest **unbound** camera. Concurrent key collisions allocate a fresh default camera, keeping both outputs bound 1:1.
   - Verified executably in `probe_sealed_key_steal` negative control and unit tests.
2. Cross-session camera persistence for ambiguous keys is gated under `[GATE G-OB48]` until platform validation.

**Verdict:** **CONFIRMED & VALIDATED (PASS).**
