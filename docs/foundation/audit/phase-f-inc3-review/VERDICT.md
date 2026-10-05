# Phase F Increment 3 (INC-3) — Independent Adversarial Systems Architecture Review Verdict

- **Review Target:** `~/infynitae-native`
- **Reviewed Commit:** `0c4e961f9d85c62dec9915f2ab0c9141afeeb897`
- **Pre-INC-3 Ratified Baseline:** `6470e1a5e22065baa940d4f5b5dd0bd2b1e281a7`
- **Review Date:** 2026-10-05
- **Role:** Independent Principal Systems Architect, Senior Rust Systems Engineer, Wayland/Smithay Compositor Engineer, and Adversarial Verification Reviewer.

---

## 1. Definitive Verdict

```
INC-3 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION
```

The core architecture, invariants, formulas, authority encapsulation, coordinate transforms, and state machines implemented in INC-3 are structurally sound, mathematically rigorous, and adhere to canonical specifications.

However, formal ratification is blocked by **one Level B finding** (`INC3-F09`):
Running `cargo test --workspace --locked` out-of-the-box on standard Linux environments with the default open-file limit (`ulimit -n 1024`) panics with `TooManyOpenFiles` during `t1_transform::ee1_c5_render_hit_agreement_on_the_production_path`. This leak prevents standard environment verification and prevents closing `OB-45`.

---

## 2. Findings Catalog

### Level A (Architectural / Invariant / Safety Violations): 0
No Level A violations were found.

### Level B (Implementation / Test-Suite Defects): 1
- **`INC3-F09` — File Descriptor Exhaustion in `t1_transform` (`ee1_c5` / `n_pan_moves_render_and_hit_together`)**
  - **Location:** `crates/infynitae-backend-headless/tests/t1_transform.rs` and `crates/infynitae-backend-headless/tests/common/mod.rs` (`Probe::buffer`).
  - **Symptom:** `cargo test --workspace --locked` fails with `shm file: Os { code: 24, kind: TooManyOpenFiles, message: "Too many open files" }` under `ulimit -n 1024`.
  - **Root Cause:** In `tile_view`, 192 subsurfaces are allocated, each creating an unlinked temporary shm file whose file descriptor is pushed to `self.files` and sent to the server. `ee1_c5` loops through 10 test cases, creating a new `Lockstep::new` on each iteration. In `wayland-server` 0.31 / `smithay`, dropping `DisplayHandle` does not immediately close or reap client shm pool fds without an explicit event loop dispatch/flush and client destroy. As a result, each iteration leaks ~193 fds. By Case 4 (iteration 5), open fds reach 781 + 193 = 974, exceeding 1024 midway through the test.
  - **Classification:** Level B (failing out-of-the-box test execution on standard developer Linux environments).

### Level C (Specification / Documentation Scoping Contradictions): 1
- **`INC3-F08` — Popup Anchor-Output Rule 1 Scoping**
  - **Location:** `docs/foundation/CHECKPOINT.md` and authorization documents.
  - **Description:** INC-3 authorization mentioned popup anchor-output rule 1 as part of INC-3, but popups, popup grabs, and `ActiveElementFocus` are scheduled for INC-4 (`OB-4`). Core currently places surfaces at (0,0) without window hierarchy or popup grabbing.
  - **Classification:** Level C (scoping alignment with `OB-4`).

### Observations and Scaffolding Ratifications: 5
- **`INC3-F01` — Modifier State Parameter in Headless Backend:** Legitimate headless test scaffolding. Core maintains no keyboard/XKB state; dynamic modifiers will enter via libinput in INC-4.
- **`INC3-F02` — Frame Invalidation on Pan:** Dropping `ProjectionFrame` during camera pan (`self.frames.retain(|(o, _)| *o != output)`) is correct and ensures true-position reconciliation immediately reads the panned camera even before the next frame is produced.
- **`INC3-F03` — Headless Vblank Frame Production:** Producing a frame using `DummyRenderer` on headless vblank records the `ProjectionFrame` before ticking the frame clock, strictly honoring `INV-HIT-1`.
- **`INC3-F06` — Efficacy Tool Shared Target Directory:** `tools/verify-test-efficacy` uses a single shared `target/test-efficacy` directory, which can cause file locking or race conditions if run concurrently.
- **`INC3-F07` — Direct Manipulation Verification (`F-PAN-1`):** `C = C_start - Q16(Σ dp_phys / (s_O * z_C))` preserves 1:1 screen-to-canvas dragging without slip across fractional scales, tested against exact x8 trace literals.

---

## 3. Obligation Adjudication

| Obligation | Description | Previous Status | Review Disposition |
|---|---|---|---|
| `OB-45` | Render and hit-test agreement under coordinate transformations | OPEN (T0 satisfied) | **OPEN — BLOCKED BY INC3-F09** (T1 implementation correct, but fd leak prevents clean PASS under default ulimit) |
| `OB-50` | Input routing through transformed layers | OPEN | **OPEN** (Core and pointer parts satisfied; surface constraints belong to INC-5) |
| `OB-3` | Output disconnect resource cleanup | OPEN (INC-2 partial) | **OPEN** (Removal and interaction orphaning satisfied; constraint reconciliation belongs to INC-5) |
| `OB-21` | Input subsystem architecture enforcement | OPEN | **OPEN** (T0/T1 complete; T4 physical libinput remains) |
| `OB-4` | Popup dismissal on unhandled click | OPEN | **OPEN** (Precondition noted; popup dismissal belongs to INC-4) |

---

## 4. Required Corrective Actions for Ratification

1. **Resolve `INC3-F09`:**
   - In `crates/infynitae-backend-headless/tests/common/mod.rs` and/or `t1_transform.rs`, optimize buffer allocation (e.g. allocate a single shared `wl_shm_pool` for the 192 subsurfaces or explicitly clean up client connections and fds on `Lockstep` drop) so `cargo test --workspace --locked` passes out-of-the-box under standard Linux `ulimit -n 1024`.
2. **Re-run Full Workspace Verification:**
   - Verify `cargo test --workspace --locked` passes without setting elevated `ulimit`.
   - Verify all 27 architecture probes, 78 efficacy controls, and document checks remain green.
3. **Commit Corrective Patch and Ratify:**
   - Commit the fix and update `docs/foundation/CHECKPOINT.md` to reflect INC-3 RATIFIED.
