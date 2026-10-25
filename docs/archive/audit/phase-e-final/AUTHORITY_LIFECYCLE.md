# Phase E Final Review — Authority Model & Output Lifecycle Audit

**Review Date:** 2026-10-05  
**Auditor:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  

---

## 1. Output Lifecycle & Camera Authority Architecture

The Infynitae architecture separates Wayland protocol dispatch from internal domain authority using typed capability tokens and linear ownership:
- **`OutputLifecycleToken`:** Minted exclusively inside `Core::new`. It is `!Send`, `!Clone`, and possesses private fields. Only backend display code holding `&mut OutputLifecycleToken` can invoke output lifecycle transitions.
- **`Authorized` Proof:** A zero-sized, `!Send`, non-`Clone` linear proof type whose constructor `mint()` is private to `core::entry`. Every camera mutation requires consuming an `Authorized` proof by value.

### 1.1 Lifecycle Entry Points and Authority Enforcement

| Entry Point | Capability Required | Mutates Camera? | Mutates Descriptor? | Clamps Pointer? |
|---|---|---|---|---|
| `output_connected` | `&mut OutputLifecycleToken` | YES (creates or rebinds) | YES (adds slot) | YES (on leaving `NoOutput`) |
| `output_disconnected` | `&mut OutputLifecycleToken` | YES (unbinds camera) | YES (removes slot) | YES (`F-RELOCATE-1` or `NoOutput`) |
| `output_reconfigured` | `&mut OutputLifecycleToken` | **NO** (identity/value kept) | YES (mode/scale/transform) | YES (`F-RECONF-1`) |
| `physical_input` | `&mut InputPort` | YES (`F-PAN-1`) | NO | NO |
| `shell_command` | `&mut ShellPort` | YES (`jump` or `fit`) | NO | NO |
| `protocol_event` | `&mut Core` only | **FORBIDDEN** (E0624) | **FORBIDDEN** | NO |

---

## 2. Independent Verification of Authority Spike (`c1-authority-spike`)

The authority spike in `experiments/c1-authority-spike/` implements the compiler-enforced boundary. Execution of `verify.sh` yielded complete verification across 13 distinct negative build probes and runtime tests:

1. **Protocol Moving Camera:** Rejected with `error[E0624]: method set_center is private`.
2. **Forging InputPort:** Rejected with `cannot construct InputPort with struct literal syntax due to private fields`.
3. **Storing Authority Port in Protocol State:** Rejected with `error[E0277]: *const () cannot be sent between threads safely`.
4. **Protocol Re-fitting Camera (PD-F2-01):** Rejected with `error[E0624]: associated function mint is private`.
5. **Core Retaining Sticky Proof:** Rejected with `error[E0277]: *const () cannot be sent between threads safely`.
6. **Output Connected Without Token:** Rejected with `error[E0061]: this method takes 2 arguments but 1 argument was supplied`.
7. **Forging OutputLifecycleToken:** Rejected with private field error.
8. **Descriptor Carrying Camera Coordinates:** Rejected with `error[E0560]: struct OutputDescriptor has no field named center`.
9. **Public Restore API:** Rejected with `error[E0599]: no method named restore_persisted_state found`.
10. **Protocol Creating Camera:** Rejected with `error[E0624]: associated function mint is private`.
11. **Reconfiguring Without Token / With Wrong Port:** Rejected with `error[E0061]` and `error[E0308]`.
12. **Token Construction Uniqueness:** Single construction site inside `Core::new` verified; negative control detects seeded `reissue_token`.
13. **Camera Mutation in `output_reconfigured`:** Source check verifies 0 calls to `mint()` and 0 calls to `navigation::`; negative control detects seeded camera jump.

---

## 3. Camera Cardinality, Collisions & Persistence Identity

### 3.1 Strict 1:1 Binding vs Mirroring (Finding E-F2-04)

Phase D contained contradictory normative statements permitting "N outputs may share one camera" while implementing a 1:1 `CameraSet.bound: Option<OutputId>`.
In E1R:
- `INV-CAMERA-3` normatively establishes:
  > *"Binding is 1:1 in Foundation 01: each connected output is bound to exactly one camera, and a camera is bound to at most one output. Shared cameras (mirroring) are not part of Foundation 01 (gate G-MIRROR)."*
- Mirroring is formally deferred behind `G-MIRROR`.

### 3.2 OutputKey Collisions & Camera Stealing Prevention (Finding E-F3-03)

`OutputKey` is derived from DRM connector facts and EDID data. Two identical displays without serial numbers produce identical `OutputKey`s.
Under stock Phase D 2R, connecting a second identical monitor stole the camera from the first monitor.
Under E1R (`navigation::bind_on_connect`):
1. A remembered camera is re-bound only if it is **currently unbound**:
   ```rust
   let free = (0..cams.key.len()).find(|&i| cams.key[i] == key && cams.bound[i].is_none());
   ```
2. If all cameras for that key are already bound (a concurrent key collision), a new default camera is created and bound to the new output.
3. Negative control probe 2i in `verify.sh` asserts that the sealed rule steals the camera and violates 1:1, whereas the E1R rule preserves 1:1.

### 3.3 Persistence Boundary Gate (G-OB48)

Physical monitors cannot be guaranteed to possess unique serial numbers (many EDIDs have null or identical serials).
- `OutputKey` serves as a *persistence identity*, not a physical monitor identity.
- In-session camera preservation (unbound on disconnect, rebound on reconnect) is fully implemented and tested.
- Cross-session persistence as a product claim is explicitly fenced behind **`G-OB48`** (`OB-48`), preventing false architectural claims of physical device tracking.
