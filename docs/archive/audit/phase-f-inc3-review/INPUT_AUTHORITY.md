# Phase F Increment 3 (INC-3) — Input Authority & Architecture Enforcement Audit

---

## 1. Input Authority Architecture

In the sealed architecture, external input events (whether from libinput, headless injection, or test rigs) represent untrusted data that must pass through an authoritative, unforgeable capability before mutating compositor state or triggering camera navigation.

### A. The `InputPort` Capability
- Defined in `crates/infynitae-core/src/authority.rs`:
  ```rust
  #[derive(Debug)]
  pub struct InputPort {
      core_id: u64,
  }
  ```
- **Creation:** Minted exactly once during `Core::new()` alongside `OutputLifecycleToken` and `ShellPort`.
- **Non-Duplicability:** Does NOT implement `Clone`, `Copy`, or `Default`. It cannot be derived or forged outside `Core::new`.
- **Binding Check:** `physical_input` asserts that `port.core_id == self.id`, rejecting foreign ports with `EntryError::ForeignPort`.

### B. Proof-Passing to `CameraSet::pan`
- Mutating a camera requires `Authorized::mint()`.
- `Authorized` is a private, zero-sized capability token in `crates/infynitae-core/src/authority.rs`.
- `Authorized::mint()` is private to `infynitae-core`.
- Inside `Core::physical_input`, a private proof is minted on demand per valid pan motion and passed into `self.cameras.pan(proof, camera, start, sum)`.
- No caller outside `Core` can call `CameraSet::pan`.

---

## 2. Compiler Probes & AST Source Rules

### A. Negative Compiler Probes (`enforcement/probes`)
- **`p18_physical_input_requires_input_port`:** Verifies that passing a dummy token or omitting `InputPort` fails compilation.
- **`p19_pan_requires_authorized_proof`:** Verifies that `CameraSet::pan` cannot be called without `Authorized`.
- **`p20_authorized_not_constructible_externally`:** Verifies that `Authorized::mint()` is not accessible outside `infynitae-core`.
- **`p21_input_port_not_clone`:** Verifies that `InputPort` cannot be cloned (`the trait bound InputPort: Clone is not satisfied`).

### B. AST Source Rules (`enforcement/source-rules`)
- **`effects-only-from-core`:**
  - Audits AST of all crates outside `infynitae-core`.
  - Rejects any code that instantiates `protocol::Effect` or imports enum variants of `Effect`.
  - Ensures effects driving Smithay can ONLY originate from Core.
- **`smithay-pointer-only-in-adapter-pointer`:**
  - Audits all crates.
  - Rejects any direct method invocation on `smithay::input::pointer::PointerHandle` or `PointerInnerHandle` outside `crates/infynitae-wayland/src/pointer.rs`.
  - Prevents backends or adapters from driving Smithay's pointer directly.

---

## 3. Seeded Control & Verification Summary

All 27 architecture probes, 58 negative compiler/AST controls, and 76 efficacy controls detect their respective mutations:
- `tools/verify-architecture: PASS`
- `tools/verify-test-efficacy: PASS`
- Tree remains byte-identical after verification.
