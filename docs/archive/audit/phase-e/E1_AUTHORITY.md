# Checkpoint E1 — Information Model Reconstruction & Authority Audit

**Reviewer:** Independent Principal Systems Architect  
**Baseline:** Phase D Final Seal (`c0a1cee310e56b75d32cf30152a09975ff776700`)

---

## 1. Information Model Reconstruction

The sealed Phase D foundation package consists of 11 markdown documents, defining 17 identifier namespaces, with exact canonical homes established in `FOUNDATION_ARCHITECTURE.md §P0`:

| Namespace | Canonical Living Home | Declared Count | Verified Independent Count | Status |
|---|---|---|---|---|
| `INV` | `FOUNDATION_INVARIANTS.md` | 57 | 57 | Verified |
| `F` | `FOUNDATION_ARCHITECTURE.md` (§F) | 19 | 19 | Verified |
| `PROV` | `FOUNDATION_ARCHITECTURE.md` (§PROV) | 11 | 11 | Verified |
| `OB` | `FOUNDATION_TEST_PLAN.md` | 44 | 44 | Verified |
| `ADR` | `FOUNDATION_DECISIONS.md` | 14 | 14 | Verified |
| `OBJ` | `FOUNDATION_OBJECT_MODEL.md` | 34 | 34 | Verified |
| `SM` | `FOUNDATION_STATE_MACHINES.md` | 22 | 22 | Verified |
| `INC` | `FOUNDATION_IMPLEMENTATION_PLAN.md` | 18 | 18 | Verified |
| `RISK` | `FOUNDATION_RISKS.md` | 66 | 66 | Verified |

### Observations on Namespace Boundaries & Historical Layers
1. **Historical vs Living Isolation:** The living regions (`<!-- living:NAME -->` ... `<!-- /living -->`) maintain lexical separation from historical Phase C, 1R, and 2R layers.
2. **Effective Invariant Registry:** Reconstructing the effective registry reveals that 39 invariants originate from C12, 12 from C12.R1, and 6 from C12.R2.
3. **Information Model Integrity Finding (E-F1-01):** In `FOUNDATION_DECISIONS.md`, all ADRs cite invariants using bare prefixes (e.g., `- **Invariants:** STAGE-1, STAGE-2`) instead of canonical `INV-*` tokens. Consequently, the reference checker in `tools/check-foundation-docs` skipped validation of every invariant citation in the ADR document.

---

## 2. Writer Graph Audit

Authoritative state writers were reconstructed across the architecture to identify multi-writer hazards, framework backdoors, and capability leakage:

| State Element | Authoritative Object | Designated Writers | Capabilities Required | Audit Finding |
|---|---|---|---|---|
| **Stage Placement (Anchors, Z-Order)** | `OBJ-01` (`Stage`) | `stage::place` callers: initial commit, interactive drag commit, shell `MoveElement` | Internal core call; drag requires `InputPort`; shell requires `ShellPort` | Single writer maintained; drag state isolated in `OBJ-26` during interaction. |
| **Camera Position & Zoom** | `OBJ-07` (`CameraSet`) | `core::navigation` only | Consumes fresh `Authorized` proof minted only in `physical_input`, `shell_command`, `output_connected` | **Sound.** Compiler probes enforce no direct access from protocol code. |
| **Camera Creation & Binding** | `OBJ-07` (`CameraSet`) | `output_connected` / `output_disconnected` | `&mut OutputLifecycleToken` | **Contradiction Found (E-F2-04).** `INV-CAMERA-3` states "N outputs may share one camera", but the 2R output authority model binds cameras 1:1 per `OutputKey` with no API or proof to share a camera across outputs. |
| **Output Lifecycle & Descriptors** | `OBJ-10` (`OutputModel`) | Backend display events via `output_connected` / `output_disconnected` | `&mut OutputLifecycleToken` | **API Gap Found (E-F2-03).** `M4` specifies a runtime "descriptor update through the token entry", but `INV-OUTPUT-2` specifies only connect and disconnect. No dynamic reconfiguration entry point exists. |
| **Persisted Cameras** | `OBJ-09` | Read-only lookup table populated at startup | Passed by value to `Core::new` | No public restore API; consulted passively only inside `output_connected`. Sound. |
| **ActiveElementFocus** | `OBJ-22` | `FocusModel` | Explicit user actions (click, binding), activation grant, target loss | Single writer. Camera motion, visibility, and protocol routing cannot mutate it. |
| **Protocol Surface Route** | `OBJ-23` | Smithay popup grab chain | Subordinate to `ActiveElementFocus` | Ordering enforced during focus preemption (INV-INPUT-10). |
| **Seat Physical Pointer** | `OBJ-20` | Core intake | `PhysicalEvent` via `InputPort` | **Crash Hazard Found (E-F2-05).** Modeled as mandatory `(interaction output, OutputLogical p)`. When 0 outputs exist, this state is unrepresentable. |
| **Pointer Constraints** | `OBJ-21` | Core activation policy + Smithay protocol object | Evaluated at batch end; requires `ActiveElementFocus` | **Protocol Flaw Found (E-F2-02).** Explicit deactivation on persistent locks leaves stale entries in Smithay, causing duplicate `unlocked` events on later focus changes. |
| **Scale Policy State** | `OBJ-16` (`ScaleState`) | Batch-end evaluation, settlement, P7 bound | Pure evaluation inside `Core` | **Division by Zero Found (E-F2-06).** `F-SCALE-2` computes `D_max / s_max(E)` without guarding `s_max(E) == 0`. |
| **Fullscreen Presentation** | `OBJ-19` | Core protocol policy | Validated `set_fullscreen` request | Output choice fixed at grant; presentation view replaces camera view without mutating camera. |

---

## 3. Capability Audit & Attack on `OutputLifecycleToken`

### Architecture Model
In Phase D 2R, `OutputLifecycleToken` was introduced to close backdoor PD-F2-18:
1. `Core::new` mints exactly one `OutputLifecycleToken` alongside `InputPort` and `ShellPort`.
2. The token has private fields, does not implement `Clone`, and is marked `!Send` via `PhantomData<*const ()>`.
3. The protocol state struct `Handlers` in `infynitae-wayland` is asserted `Send`, creating an E0277 compiler rejection if the token is stored in protocol state.
4. `verify-architecture` step 2g checks that only `Core::new` constructs the token.

### Adversarial Attack Vector 1: Token Storage & Lifetime in Compositor Backend
In a real compositor process, `OutputLifecycleToken` must be stored across the entire session to service hotplug events. Because it is `!Send`, it must remain on the thread that runs the display/output backend. In Smithay single-threaded event loops (using `calloop`), `State` holds `Core`, while the backend event source closure captures `OutputLifecycleToken`. Because `infynitae-wayland` has an architecture rule forbidding naming `OutputLifecycleToken`, protocol handlers cannot borrow the token from the backend. The capability separation holds at compile time.

### Adversarial Attack Vector 2: Reconfiguration via `OutputLifecycleToken` (Finding E-F2-03)
When attacking runtime monitor reconfiguration (resolution changes, scale adjustments, refresh rate switching), the token model breaks down:
- `INV-OUTPUT-2` explicitly defines only:
  - `output_connected(&mut OutputLifecycleToken, OutputDescriptor)`
  - `output_disconnected(&mut OutputLifecycleToken, OutputId)`
- Neither entry takes an existing `OutputId` with a new `OutputDescriptor`.
- In `c1-authority-spike`, `output_connected` unconditionally increments `next_output += 1` and creates a duplicate output entry.
- `M4` row 2 claims: `mode/scale/transform change (descriptor update through the token entry)`.
- If an output must be disconnected and reconnected to change resolution, `INV-OUTPUT-1` triggers: fullscreen is dropped, physical pointer is relocated, constraints are ended, and `wl_surface.leave` is sent to all clients.
- **Verdict:** Material architectural gap. A dedicated `output_reconfigured(&mut OutputLifecycleToken, OutputId, OutputDescriptor)` entry point is required.

### Adversarial Attack Vector 3: Camera Sharing Across N Outputs (Finding E-F2-04)
`INV-CAMERA-3` normatively states: *"N outputs may share one camera."*
However:
- `output_connected` internally creates or re-binds cameras based solely on `OutputKey` (connector + EDID).
- In `CameraSet` (`c1-authority-spike`), camera binding is stored as `bound: Vec<Option<u64>>`—allowing exactly one output per camera.
- There is no method on `Core`, no shell command in `ShellCommand`, and no capability token that permits binding output `O_2` to existing camera `C_1`.
- **Verdict:** Normative invariant `INV-CAMERA-3` is contradicted by the 2R authority model and its backing data structures.
