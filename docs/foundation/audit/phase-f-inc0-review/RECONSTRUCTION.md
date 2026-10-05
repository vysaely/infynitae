# Independent Reconstruction of INC-0 Requirements

## 1. What INC-0 Was Required to Create
Derived from `FOUNDATION_IMPLEMENTATION_PLAN.md` (INC-0), `FOUNDATION_TEST_PLAN.md` (OB-19, OB-36), and `FOUNDATION_ARCHITECTURE.md` (§C1, §OB-19):
1. **Production Cargo Workspace:** Root `Cargo.toml`, `rust-toolchain.toml` (pinned `1.99.0`), and tracked `Cargo.lock`.
2. **Crate Skeletons:** `infynitae-core`, `infynitae-wayland`, `infynitae-backend-headless`, `infynitae-backend-winit`, `infynitae-ipc`, and `infynitae` (bin).
3. **Authority Types & Skeletons:**
   - `InputPort`, `ShellPort`, `OutputLifecycleToken` (private fields, unforgeable, not `Clone`, `!Send`, single construction site).
   - `Authorized` capability proof (`!Send`, not `Clone`, `mint` private to `entry`).
   - `navigation::CameraSet` (read accessor `center`, private mutator `set_center`).
   - `output::{OutputDescriptor, OutputKey, OutputId, Transform}` (descriptors carrying hardware facts only, no coordinate/camera fields).
   - `Core::new` returning Core and ports/token.
   - `WaylandState` protocol state with static `Send` assertion.
4. **Architecture Verification Tooling:**
   - `tools/verify-architecture` entry point (deterministic, non-interactive, headless, failing closed).
   - Negative compile probes ported from `experiments/c1-authority-spike/`.
   - Seeded negative controls verifying that the verifier catches broken enforcement.
5. **CI Integration (OB-36 & OB-19):**
   - CI workflow running `tools/check-foundation-docs`, `tools/check-foundation-docs --self-test`, `tools/verify-architecture`, and workspace build/test.
6. **Environment Verifier Update:**
   - Update `tools/verify-development-environment` to recognize the legitimate Phase F workspace.

## 2. What INC-0 Was Forbidden to Create
Derived from `FOUNDATION_IMPLEMENTATION_PLAN.md` (INC-0) and prompt §4:
- No Stage or spatial canvas storage.
- No window mapping or element management.
- No rendering or hit testing.
- No input dispatch, pointer routing, or keyboard focus.
- No camera navigation (pan, zoom, fit, jump).
- No output storage, hotplug, or dynamic reconfiguration logic.
- No camera creation or output-to-camera binding logic.
- No persistence tables or persistence restoration.
- No Wayland protocol event dispatch or Smithay event loop integration.
- No GPU / DRM / KMS / libseat interaction.

## 3. OB-19 Current Criteria
1. Dependency DAG checks: `infynitae-core` has zero dependencies; all edges point to core; no forbidden edges (`wayland -> backend`, `ipc ↔ wayland`, `backend ↔ backend`).
2. Camera authority compile probes with asserted diagnostics:
   - `probe_protocol_moves_camera` (E0624)
   - Protocol-side paths: `p1` (E0624), `p2` (private fields), `p3` (E0061), `p4` (E0603).
   - Positive control: `p5` compiles.
3. Protocol state isolation: `!Send` port/token + `Send` state assertion (`probe_store_port_in_state` -> E0277).
4. 1R amendments:
   - `probe_protocol_refits_camera` (E0624 on `mint`).
   - `probe_sticky_fit` (E0277).
   - Source rule: `Authorized::mint` appears only in the entry module.
5. 2R amendments:
   - `p6` (E0061), `p7` (private fields), `p8` (E0560), `p9` (E0599), `probe_protocol_creates_camera` (E0624), `probe_store_output_token_in_state` (E0277).
   - Single construction site for `OutputLifecycleToken` in `Core::new`.
   - Source rule: `infynitae-wayland` never names `OutputLifecycleToken`, `output_connected`, or `output_disconnected`.
   - Ports, token, proof are not `Clone` (C12.R2.3).
6. E1R amendments:
   - Step 2h: reconfiguration probes `p10` (E0061), `p11` (E0308), `p12` (E0560), `p13` (E0061); source rule that `output_reconfigured` carries no camera authority.
   - Step 2i: OutputKey collision negative control (sealed 2R rule failing 1:1 test).
7. Unsafe code policy: `#![forbid(unsafe_code)]` in core and ipc; `#![deny(unsafe_code)]` elsewhere.
8. Pinned dependencies: `smithay = "=0.7.0"`, `rust-toolchain.toml` pinned (`1.99.0`), `cargo build --locked`.
9. Retained negative controls proving verifier detects defects.
10. CI-blocking enforcement.

## 4. OB-36 Exact Criterion
`FOUNDATION_TEST_PLAN.md`: A CI job executes `tools/check-foundation-docs` and `tools/check-foundation-docs --self-test` on every commit; any failure blocks merge. Separate check from OB-19.
