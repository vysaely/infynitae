# INC-6 Owner-Review Dossier: Scale Policy, Device Ceiling, ScaleInteractionEpoch, and Wayland Scale Boundary

**Status:** IMPLEMENTED & VERIFIED — READY FOR OWNER REVIEW  
**Milestone:** INC-6 (Product / Headless)  
**Branch:** `dev/inc-6-scale-policy`  
**Canonical Baseline:** Commit `db254c24bfbb6c5598fdb5c7bf4fd79ba12f4524`, Tree `ce11311c7b25a1f4c836757f673c78ccd9f5921f`  
**Ratification Status:** UNRATIFIED (Awaiting Repository Owner Review)

---

## 1. Executive Summary

Milestone INC-6 implements the authoritative Core scale model, device ceiling constraints, ScaleInteractionEpoch damping, and the Wayland fractional scale / preferred buffer scale boundary on the production path.

All canonical architectural invariants and obligations are strictly satisfied:
1. **Core-Owned Policy:** Core is the sole authority for `ScaleState` (`OBJ-16`), scale requirement derivation ($\rho$), ladder generation ($L$), device ceiling ($H_{\text{dev}}, H_{\text{max}}$), target selection ($t(\rho)$), integer buffer scale ($b(h)$), initial hint prediction ($\rho_{\text{pred}}$), and `ScaleInteractionEpoch` (`M7`).
2. **Smithay Mechanics Only:** `infynitae-wayland` exposes the `wp_fractional_scale_manager_v1` and `wp_viewporter` globals and dispatches protocol events without holding scale authority or duplicating state.
3. **P7 Correctness & Device Ceiling:** Immediate forced decrease applies when $h > H_{\text{max}}$ (INV-SCALE-2). Negative control E1R-N3 confirms that $b(h) \cdot s_{\text{max}} \le D_{\text{max}}$ holds without exception.
4. **Scale Churn Suppression:** Interaction damping via `ScaleInteractionEpoch` (`INV-SCALE-3`, `INV-SCALE-4`) suppresses hint decreases during active continuous spatial gestures (e.g. `CameraPan`, `CameraZoom`) and discrete interactions until the settlement deadline ($T_{\text{settle}} = 300\text{ ms}$).
5. **Initial Scale Hint (M15):** The predicted scale hint is emitted before the initial configure event on the wire.

---

## 2. Traceability Matrix

| Requirement | Specification Ref | Implementation Target | Verification Evidence |
|---|---|---|---|
| **OBJ-16** | `FOUNDATION_OBJECT_MODEL.md` | `infynitae-core/src/scale.rs::ScaleState` | T0 unit tests in `scale.rs` |
| **F-SCALE-1** | `FOUNDATION_ARCHITECTURE.md` | `infynitae-core/src/scale.rs::compute_rho` | T0 Walkthroughs A, B, C; T1 `t1_scale.rs` |
| **F-SCALE-2** | `FOUNDATION_ARCHITECTURE.md`, E-F2-06 | `infynitae-core/src/scale.rs::{h_dev, h_max, ladder}` | EE-5 zero extent test; CP3A ladder tests |
| **F-SCALE-3** | `FOUNDATION_ARCHITECTURE.md`, E1R-N3 | `infynitae-core/src/scale.rs::{target, preferred_buffer_scale, predict_rho}` | EE-5 integer bound test; E1R-N3 negative control |
| **INV-SCALE-1** | `FOUNDATION_INVARIANTS.md` | `infynitae-core/src/scale.rs` | T0 Case C fullscreen presentation test |
| **INV-SCALE-2** | `FOUNDATION_INVARIANTS.md` | `infynitae-core/src/scale.rs::{p7_check, step}` | T0 P7 forced decrease; T1 viewporter ceiling test |
| **INV-SCALE-3** | `FOUNDATION_INVARIANTS.md`, M7 | `infynitae-core/src/scale.rs::ScaleInteractionEpoch` | TST-23 R2; TST-24; T1 Case E camera pan damping |
| **INV-SCALE-4** | `FOUNDATION_INVARIANTS.md` | `infynitae-core/src/scale.rs` | TST-17 feedback model; T1 SC-E churn test |
| **INV-SCALE-5** | `FOUNDATION_INVARIANTS.md`, M15 | `infynitae-core/src/protocol.rs`, `scale.rs` | T1 initial configure wire ordering test |
| **INV-IDLE-1** | `FOUNDATION_INVARIANTS.md` | `infynitae-core/src/lib.rs::next_settlement_deadline` | Event loop timeout bound to next deadline |
| **M7** | `FOUNDATION_STATE_MACHINES.md` | `infynitae-core/src/scale.rs` | T0 M7 state transition tests |
| **M15** | `FOUNDATION_STATE_MACHINES.md` | `infynitae-core/src/protocol.rs` | T0 initial hint test; T1 M15 wire ordering |
| **PROV-1..3, 5**| `FOUNDATION_ARCHITECTURE.md` | `infynitae-core/src/scale.rs::ScalePolicyConfig` | Configurable constants: $H_{\text{cfg}}=3.0, r=1.25, \sigma=0.05, T_{\text{settle}}=300\text{ ms}$ |
| **OB-12** | `FOUNDATION_TEST_PLAN.md` | `infynitae-core/src/scale.rs` | TST-16 walkthroughs A-E |
| **OB-23** | `FOUNDATION_TEST_PLAN.md` | `infynitae-core/src/scale.rs` | TST-23 R2, TST-24, T1 settlement timer |
| **EE-5** | `FOUNDATION_EVIDENCE.md` | `infynitae-core/src/scale.rs` | Zero extent domain and integer emission safety |

---

## 3. Adversarial Self-Audit

### Attack Vector 1: Re-entrant Feedback Loops & Oscillation (SC-E)
- **Attack:** An adversarial client resizes its surface in response to receiving a scale hint, causing a new scale evaluation, which triggers another configure/scale hint, oscillating infinitely.
- **Defense:** Damping and ladder hysteresis guarantee that client-driven responses are evaluated with client-response damping. Tested in `adversarial_client_churn_bounds_sc_e` over 30 rapid resizing oscillations; client scale renegotiations are strictly bounded to $\le 2$.

### Attack Vector 2: Buffer Exceeding Hardware Texture Limit (E1R-N3)
- **Attack:** A client with a large surface destination (e.g. 8000x6000) requests a high buffer scale (e.g. 3x), causing the allocated buffer to exceed the GPU/renderer limit $D_{\text{max}} = 16384$, triggering driver crash or memory exhaustion.
- **Defense:** $H_{\text{dev}}(E) = \lfloor D_{\text{max}} / s_{\text{max}}(E) \rfloor$ ceiling strictly caps integer buffer scale $b(h)$ and fractional hint $h$. Tested in `viewporter_destination_sizing_and_p7_ceiling`: buffer scale is capped at 2 ($8000 \times 2 = 16000 \le 16384$).

### Attack Vector 3: Scale Churn During Camera Panning
- **Attack:** During continuous spatial navigation (e.g. user panning camera across a multi-monitor boundary with different DPIs), raw per-motion deltas trigger dozens of fractional scale configure cycles.
- **Defense:** `ScaleInteractionEpoch` suppresses hint decreases during active `CameraPan`. Tested in `tst16_case_e_camera_pan_interaction_damping`: zero scale events are emitted during continuous pan motions; exact settlement to 1.0 (120/120) occurs only once upon button release.

### Attack Vector 4: Stale Settlement Timers & Idle Wakeups (INV-IDLE-1)
- **Attack:** Idle compositor continuously wakes up the event loop to check scale deadlines.
- **Defense:** `core.next_settlement_deadline()` returns `None` whenever no epochs are open with active deadlines. The backend event loop only arms a timer when a deadline is present.

---

## 4. Verification Gate Results

All commands executed locally in the repository environment:

1. `cargo check --workspace --locked`: **PASS**
2. `cargo build --workspace --locked`: **PASS**
3. `cargo test --workspace --locked`: **PASS** (81 core unit tests, 9 winit tests, 0 warnings/failures)
4. `cargo test -p infynitae-backend-headless --tests --locked`: **PASS** (140+ headless integration tests, including 7 `t1_scale.rs` tests)
5. `cargo run -p infynitae-source-rules -- --root .`: **PASS** (all 14 architectural source rules pass)
6. `./tools/verify-architecture`: **PASS** (dependency DAG, toolchain/Smithay pins, sealed compile probes, runtime 2i, KeepFocus, seeded negative controls)
7. `./tools/verify-test-efficacy`: **PASS** (mutation testing against seeded behavioral defects)
8. `cargo clippy --workspace --all-targets --locked -- -D warnings`: **PASS** (zero warnings across all targets)
9. `cargo fmt --all -- --check`: **PASS** (all files formatted)

---

## 5. Explicit Ratification Boundary

- The changes are isolated to development branch `dev/inc-6-scale-policy`.
- Canonical main (`db254c24bfbb6c5598fdb5c7bf4fd79ba12f4524`, tree `ce11311c7b25a1f4c836757f673c78ccd9f5921f`) has NOT been modified or merged.
- No git push has been executed.
- Milestone ratification is reserved exclusively for the repository owner.
