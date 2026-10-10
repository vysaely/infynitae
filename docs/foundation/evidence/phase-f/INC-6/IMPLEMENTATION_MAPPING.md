# INC-6 Implementation Mapping & Authority Model (Planning Record)

Written before any INC-6 production change.
Baseline: `db254c24bfbb6c5598fdb5c7bf4fd79ba12f4524` (tree: `ce11311c7b25a1f4c836757f673c78ccd9f5921f`, Phase F INC-0 through INC-5 RATIFIED).
Branch: `dev/inc-6-scale-policy`.

Canonical Category: `PRODUCT`
Execution Class: `HEADLESS`
Title: `Scale policy, device ceiling, ScaleInteractionEpoch, and Wayland fractional scale`

---

## 1. Requirements Traceability Matrix

| Requirement ID | Description | Architectural Location | Implementation Target | Verification / Test Target |
|---|---|---|---|---|
| **OBJ-16** | `ScaleState (h, ladder, ScaleInteractionEpoch{open, kind, direction, deadline})` | `FOUNDATION_OBJECT_MODEL.md` § OBJ-16 | `crates/infynitae-core/src/scale.rs` | T0 property unit tests (`t0_scale_state_invariants`) |
| **F-SCALE-1** | Scale requirement $\rho(E) = \max_{p} (\text{scale}_p \cdot \text{zoom}_p)$; fullscreen presentation zoom 1 | `FOUNDATION_ARCHITECTURE.md` § F-SCALE-1 | `crates/infynitae-core/src/scale.rs::compute_rho` | T0 CP3A walkthroughs A, B, C; T1 `t1_scale.rs` |
| **F-SCALE-2** | Device ceiling $H_{\text{max}}(E) = \min(H_{\text{cfg}}, H_{\text{dev}}(E))$, where $H_{\text{dev}}(E) = D_{\text{max}} / s_{\text{max}}(E)$ when $s_{\text{max}} > 0$ and $+\infty$ when $s_{\text{max}} = 0$; Ladder $L(E)$ | `FOUNDATION_ARCHITECTURE.md` § F-SCALE-2, E-F2-06 | `crates/infynitae-core/src/scale.rs::{h_dev, h_max, ladder}` | EE-5 zero extent domain; T0 CP3A ladder tests |
| **F-SCALE-3** | Target $t(\rho) = \text{smallest rung} \ge \rho / (1 + \sigma)$ else $H_{\text{max}}$; integer emission $b(h) = \lceil h \rceil$ if $\lceil h \rceil \le H_{\text{dev}}$ else $\max(1, \lfloor H_{\text{dev}} \rfloor)$; initial hint $h := t(\rho_{\text{pred}})$ | `FOUNDATION_ARCHITECTURE.md` § F-SCALE-3, E1R-N3 | `crates/infynitae-core/src/scale.rs::{target, preferred_buffer_scale, predict_rho}` | EE-5 integer bound test; E1R-N3 negative control; T0 initial hint test |
| **INV-SCALE-1** | $\rho(E)$ max over observing presentations; protocol-fullscreen uses zoom 1; no fullscreen override | `FOUNDATION_INVARIANTS.md` § INV-SCALE-1 | `crates/infynitae-core/src/scale.rs` | T0 Case C fullscreen presentation test |
| **INV-SCALE-2** | Emitted hint is a ladder rung; bounded by $H_{\text{dev}}(E)$; P7 immediate forced decrease when $h > H_{\text{max}}$ | `FOUNDATION_INVARIANTS.md` § INV-SCALE-2 | `crates/infynitae-core/src/scale.rs::{p7_check, step}` | T0 P7 forced decrease; T1 transition to first buffer |
| **INV-SCALE-3** | `ScaleInteractionEpoch`: kinds CameraPan, CameraZoom, WindowMove, WindowResize, ShellStream; decreases suppressed; increases only at rungs in direction; P7 immediate down; settlement at end or $T_{\text{settle}}$ | `FOUNDATION_INVARIANTS.md` § INV-SCALE-3, `FOUNDATION_STATE_MACHINES.md` M7 | `crates/infynitae-core/src/scale.rs::ScaleInteractionEpoch` | TST-23 R2; TST-24 (2R model); T0 discrete timeout test |
| **INV-SCALE-4** | Scale churn contract: no change per raw spatial event; at most rungs crossed + 1 settlement + one P7 per non-client stimulus; SC-E monotonicity | `FOUNDATION_INVARIANTS.md` § INV-SCALE-4 | `crates/infynitae-core/src/scale.rs` | TST-17 (feedback model); TST-24 drag churn test |
| **INV-SCALE-5** | MaxGesture at zoom 1; integer hint bounded by $H_{\text{dev}}$; emission only on change; unobserved hold $h$; batch-end evaluation; initial hint $h := t(\rho_{\text{pred}})$ before initial configure | `FOUNDATION_INVARIANTS.md` § INV-SCALE-5, M15 | `crates/infynitae-core/src/protocol.rs`, `scale.rs` | TST-03 SC2 production parity; T1 initial configure ordering |
| **INV-IDLE-1** | Settlement timer armed ONLY while epoch is open with discrete deadline; idle compositor has 0 wakeups | `FOUNDATION_INVARIANTS.md` § INV-IDLE-1 | `crates/infynitae-core/src/lib.rs::next_settlement_deadline` | T0 idle timer test; T1 discrete pause test |
| **M7** | ScaleInteractionEpoch state transitions | `FOUNDATION_STATE_MACHINES.md` M7 | `crates/infynitae-core/src/scale.rs` | T0 M7 transition tests; negative controls |
| **M15** | Scale hint state transitions | `FOUNDATION_STATE_MACHINES.md` M15 | `crates/infynitae-core/src/protocol.rs`, `entry.rs` | T0 M15 initial hint test; hotplug ladder update test |
| **PROV-1..3, 5** | Provisional candidates: $H_{\text{cfg}} = 3.0$, $r = 1.25$, $\sigma = 0.05$, $T_{\text{settle}} = 300\text{ ms}$; configurable, not frozen | `FOUNDATION_ARCHITECTURE.md` § PROV | `crates/infynitae-core/src/scale.rs::ScalePolicyConfig` | Parameterised test over allowed ranges |
| **OB-12** | Client scale ceiling $H_{\text{cfg}}$, ratio $r$, tolerance $\sigma$ | `FOUNDATION_TEST_PLAN.md` § OB-12 | `crates/infynitae-core/src/scale.rs` | TST-16 walkthroughs A-E |
| **OB-23** | $T_{\text{settle}}$ discrete interaction renegotiation rate | `FOUNDATION_TEST_PLAN.md` § OB-23 | `crates/infynitae-core/src/scale.rs` | TST-23 R2, TST-24 |
| **EE-5** | Zero extent domain ($s_{\text{max}} = 0 \to H_{\text{max}} = H_{\text{cfg}}$) and integer emission within $D_{\text{max}}$ | `FOUNDATION_EVIDENCE.md` EE-5 | `crates/infynitae-core/src/scale.rs` | E-F2-06 and E1R-N3 tests with negative controls |

---

## 2. Authority & Module Responsibilities

1. **`infynitae-core`**:
   - Sole authority for `ScaleState` (`OBJ-16`), scale requirement derivation ($\rho$), ladder generation ($L$), device ceiling ($H_{\text{max}}$), target selection ($t(\rho)$), integer buffer scale ($b(h)$), initial hint prediction ($\rho_{\text{pred}}$), and `ScaleInteractionEpoch` (`M7`).
   - Module: `crates/infynitae-core/src/scale.rs`.
   - Produces `Effect::PreferredScale { element, scale_120, integer_scale }`.
   - Never depends on Smithay or any external crate (std only).

2. **`infynitae-wayland`**:
   - Advertises `wp_fractional_scale_manager_v1` and `wp_viewporter` globals.
   - Manages Smithay protocol objects: `wp_fractional_scale_v1`, `wp_viewport`.
   - Consumes `Effect::PreferredScale` in `outputs.rs` / `convert.rs`, applying `set_preferred_scale` and `send_surface_state` to all surfaces of the element.
   - Never decides scale values or computes ladders.

3. **`infynitae-backend-headless` / `infynitae-backend-winit`**:
   - Ingests renderer capability ($D_{\text{max}}$) into Core.
   - For `DummyRenderer`, $D_{\text{max}} = 16384$.
   - Integrates timer wakeups for discrete settlement deadlines when `core.next_settlement_deadline()` is armed.
