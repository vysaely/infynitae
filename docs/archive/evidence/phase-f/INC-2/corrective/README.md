# INC-2 Corrective Verification and Ratification

## History

| Step | Commit / record | Result |
|---|---|---|
| INC-2 implementation | `0a03aab` (code), `8f27a31` (evidence) | Implemented; OB-49 OPEN pending review |
| Independent review | Dossier [`audit/phase-f-inc2-review/`](../../../../audit/phase-f-inc2-review), recorded unchanged in `b229462` | **INC-2 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION**; INC2-F07 (Level B), INC2-F01 (Level C) |
| Corrective patch | `fix(test): close INC2-F07 reconfiguration frame efficacy gap` | Tests and mutation controls only; production code unchanged |
| Corrective verification | Outputs in this directory | All PASS (below) |
| Ratification | `docs(foundation): ratify INC-2 and correct OB-49 ownership` | OB-49 `CLOSED` via Decision E; INC-2 ratified |

The review's verdict and findings stand as written. This record does not replace them.

---

## INC2-F07 — Output Reconfiguration Frame Invalidation Efficacy Gap (Level B): RESOLVED

### Root Cause
In `Core::output_reconfigured`, line 120 purges the output's cached `ProjectionFrame`:
```rust
self.frames.retain(|(o, _)| *o != output);
```
However, no test in the test suite previously called `Core::hit_test` or asserted frame cache state after `output_reconfigured`. If line 120 were deleted or neutralized, all 75 workspace tests and all 45 existing test-efficacy controls still passed.

### Why Stale ProjectionFrame Survival Is Invalid
An output's `ProjectionFrame` caches the screen-space bounding boxes and geometry entries of elements for physical hit-testing and presentation based on the output's mode, transform, and scale at the moment the frame was rendered (`INV-HIT-1`, `F-HIT-1`, `F-VIEW-1`).
When an output reconfigures (mode resolution changes, transform rotates, or scale changes):
1. Retaining a stale `ProjectionFrame` allows `Core::hit_test` to evaluate coordinates against obsolete view extents, transforms, and surface origins until a new frame happens to be rendered.
2. If the output shrinks in resolution, clicks in newly out-of-bounds regions could hit stale elements.
3. If the output rotates (e.g. 90° turn), coordinates mapped in view space would hit surfaces stacked according to the pre-rotation orientation, violating `INV-HIT-1` ("exact physical-pixel hit-testing against presented geometry").
Therefore, purging the cached `ProjectionFrame` immediately upon reconfiguration is mandatory.

### Correction
Added permanent test `reconfiguration_drops_the_output_frame` in `crates/infynitae-core/src/entry.rs`:
- Creates an element and connects an output (1920x1080, Normal, scale 1.0).
- Renders a frame, verifying that a point hits the element.
- Reconfigures the output with a mode shrink (800x600), transform change (90° turn / R90), and scale change (2.0).
- Verifies that:
  1. The frame cache is immediately purged;
  2. `Core::hit_test` immediately returns `None` (failing closed rather than returning stale hit results) before any new frame is rendered.

### Permanent Mutation Control
Added mutation control `INC2-F07: reconfiguration keeps the output's ProjectionFrame` to `tools/verify-test-efficacy`:
- Replaces `self.frames.retain(|(o, _)| *o != output);` with `let _ = output;` in `crates/infynitae-core/src/entry.rs`.
- Result: **DETECTED** by `infynitae-core --lib reconfiguration_drops_the_output_frame`.

---

## Decision E — OB-49 Ownership Correction (Level C / Finding INC2-F01): RATIFIED

### Rationale
In `docs/foundation/FOUNDATION_TEST_PLAN.md`, the historical "Item:" and "Pass criterion:" texts of obligation `OB-49` mixed canonical INC-2 output lifecycle requirements with obligations belonging to later increments:
- **Protocol fullscreen configure:** Canonically belongs to INC-7 (`INV-PRES-1`, `OB-22`, `OB-32`).
- **ScaleInteractionEpoch settlement:** Canonically belongs to INC-6 (`ScaleInteractionEpoch`, `OB-13`, `OB-47`).

Neither protocol fullscreen configure nor epoch settlement can be executed or tested in INC-2 (which is headless output lifecycle infrastructure without client input routing, gesture interaction, or xdg toplevel configure integration).

### Corrected Ownership
Decision E formally corrects OB-49 ownership without weakening any architectural invariant:
- **INC-2 / OB-49 owns:**
  - `output_reconfigured` for mode, transform, scale, and refresh changes;
  - Same `OutputId`;
  - Camera identity and camera values unchanged across reconfiguration;
  - Rebuilding damage tracker, view extent, and surface membership;
  - `F-RECONF-1` pointer clamping;
  - `wl_output` and `xdg_output` wire protocol events;
  - Rejection of unknown ids and key changes without mutation;
  - Disconnect and reconnect tested separately;
  - `ProjectionFrame` dropped on reconfiguration (closing `INC2-F07`);
  - Runtime 2i validation (transferred from OB-19 via Decision D): runtime 1:1 camera/output binding preservation and `OutputKey` collision negative control (`probe_sealed_key_steal` / `concurrent_output_key_collision`), proving that `INV-CAMERA-3` and `INV-OUTPUT-2` hold during multi-output binding.
- **Transferred to INC-7:** Protocol fullscreen configure (`INV-PRES-1`, `OB-22`, `OB-32`).
- **Transferred to INC-6:** `ScaleInteractionEpoch` settlement (`ScaleInteractionEpoch`, `OB-13`, `OB-47`).

### Final OB-49 Status
Every criterion remaining under OB-49 after Decision E has complete mechanical and test evidence in the repository.
Therefore, `OB-49` is formally ratified as **`CLOSED`**.

---

## Tracking of Remaining Open Obligations

| Obligation | Scope / Description | Current Status | Assigned Owner Increment / Tier |
|---|---|---|---|
| `OB-49` | Output reconfiguration & runtime 2i | **CLOSED** | Ratified in INC-2 via Decision E |
| `OB-45` | Render / hit agreement across output transforms | OPEN | INC-2 tracker/membership portion satisfied; hit-test portion owned by INC-3 |
| `OB-50` | Zero-output lifecycle | OPEN | INC-2 core `NoOutput` satisfied; adapter pointer grab in INC-3; constraint release in INC-5 |
| `OB-3` | Output removal without interaction state machine | OPEN | INC-2 output removal satisfied; interaction orphaning owned by INC-3 |
| `OB-1` | Lattice reference vs production rasterization | OPEN | INC-6 / T1 |
| `OB-6` | Fractional scale edge rounding parity | OPEN | INC-6 / T4 |
| `OB-11` | Dependency patch P1 (Smithay `leave()` constraint) | OPEN | INC-5 gate |
| `OB-14` | Subsurface unmap / sync commits | OPEN | INC-9 / T1 |
| `OB-17` | Direct-scanout candidate evaluation | OPEN | INC-16 / T4 |
| `OB-48` | Multi-session persisted camera storage | OPEN | INC-13 / T1 |

---

## Verification Summary (Outputs in this Directory)

- **Cargo:**
  - `cargo check --workspace --locked`: PASS
  - `cargo build --workspace --locked`: PASS
  - `cargo test --workspace --locked`: PASS (76 tests: 53 core, 19 distinct T1 + 4 self-tests)
  - `cargo fmt --all -- --check`: PASS
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS
- **`tools/verify-architecture`:** PASS
  - 27 compile probes asserting exact diagnostics;
  - 47 seeded controls detected (including F01/F02 regressions);
  - Runtime 2i negative probe verified.
- **`tools/verify-test-efficacy`:** PASS
  - 46/46 seeded behavioural defects detected (including INC2-F07);
  - Judge controls verified;
  - Canonical tree byte-identical after run.
- **Foundation Documentation:**
  - `tools/check-foundation-docs`: PASS (57 invariants, 20 formulas, 51 obligations intact);
  - `tools/check-foundation-docs --self-test`: PASS (18/18 seeded defects detected).
- **Historical C1 Spike:**
  - `experiments/c1-authority-spike/verify.sh`: PASS.
- **Development Environment:**
  - `tools/verify-development-environment`: PASS.
