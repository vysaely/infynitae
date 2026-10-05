# Phase D corrective checkpoint 2R — invariant diff

**Input layers:**
- the sealed registry C12 (`615b0ed`);
- the 1R amendments C12.R1 (`84ce620`);
- the 2R trigger findings PD-F2-18 and PD-F2-19 (FOUNDATION_BLOCKERS.md § Phase D Checkpoint 2). These are recorded in the uncommitted working tree on top of `2ee8ab6`.

**Normative through:** FOUNDATION_ARCHITECTURE §C12.R2. The C12 and C12.R1 tables are left unaltered.

**Result:** 5 invariants amended (INV-CAMERA-1, INV-CAMERA-3, INV-OUTPUT-1, INV-SCALE-3, INV-SCALE-4) and 1 new (**INV-OUTPUT-2**). The registry now has **57** entries.

**Evidence:**
- **ER2-1:** authority spike, 2R. New probes p6–p9, `probe_protocol_creates_camera` and `probe_store_output_token_in_state`, plus 3 new behaviour tests. [`authority-spike-2r-verify.txt`](../../evidence/phase-d/authority-spike-2r-verify.txt)
- **ER2-2:** `d5_model_2r` (MODEL), with a negative control. [`d5-model-2r-output.txt`](../../evidence/phase-d/d5-model-2r-output.txt)

Each entry: previous wording (verbatim), trigger, replacement, why necessary, evidence, ADR, tests/obligations.

---

### INV-CAMERA-1 (amended again)
- **Previous (C12.R1 @ `84ce620`):** "Camera state is mutated only inside `core::navigation`. Each mutation consumes a fresh `Authorized` proof, mintable only inside the port-carrying entry points (`physical_input(&mut InputPort)`, `shell_command(&mut ShellPort)`); the proof is not Clone, `!Send`, and consumed by value. Core retains no proof, mode, target or tracking state permitting a later mutation. No protocol event or client-originated change causes a camera mutation. Ports are unforgeable, `!Send`, bound to their Core, never stored in protocol state. Animation is not in Foundation 01; any later time-driven step must be an authorized action with its own proof."
- **Trigger:** PD-F2-18.
- **Replacement:**
  - Camera state is mutated only inside `core::navigation`. Each mutation, **including camera creation and output↔camera binding or unbinding**, consumes a fresh `Authorized` proof.
  - A proof is mintable only inside three authority entry points:
    - `physical_input(&mut InputPort)`;
    - `shell_command(&mut ShellPort)`;
    - `output_connected` / `output_disconnected(&mut OutputLifecycleToken)` (INV-OUTPUT-2).

    The proof is not Clone, `!Send`, and consumed by value.
  - The output-lifecycle entries may only create, bind or unbind cameras. They never take a camera position, centre or zoom from their caller, and never change the value of an existing camera.
  - Core retains no proof, mode, target or tracking state permitting a later mutation. No protocol event or client-originated change causes a camera mutation.
  - Ports and tokens are unforgeable, `!Send`, bound to their Core, and never stored in protocol state.
  - Animation is not in Foundation 01; any later time-driven step must be an authorized action with its own proof.
- **Why necessary:** under the 1R wording, camera creation (output connect), binding and persistence restore had no authorization path. The natural port-free `output_event` entry would give protocol code (which holds `&mut Core`) a compile-time path to create cameras and re-bind which camera an output shows.
- **Evidence (ER2-1):**
  - `probe_protocol_creates_camera` fails with E0624 (`mint` private).
  - p6 (`output_connected` without the token) fails with E0061.
  - Test `ports_are_bound_to_their_core`: a foreign token is rejected and no camera is created.
- **ADR:** ADR-08, ADR-04.
- **Tests/obligations:** OB-19, probe list extended (2f).

### INV-CAMERA-3 (amended)
- **Previous (C12 @ `615b0ed`):** "Cameras are centre-anchored views (centre Q16, zoom); per-output view origin `v_O = centre − mode/(2k)` (R7); cameras survive output removal; N outputs may share one camera"
- **Trigger:** PD-F2-18.
- **Replacement:**
  - Cameras are centre-anchored views (centre Q16, zoom); per-output view origin `v_O = centre − mode/(2k)` (R7).
  - **Cameras come into existence only through `output_connected` (INV-OUTPUT-2).** For the connecting output's `OutputKey`, the camera is the first of:
    1. the camera remembered this session for that key, re-bound with its value unchanged;
    2. a new camera seeded from the passive persisted lookup entry for that key;
    3. a new camera at Core's internal default placement (the placement policy itself is Level D).
  - Cameras survive output removal (unbound, value kept) and are not destroyed in-session. N outputs may share one camera.
- **Why necessary:** camera creation, binding and the remembered-versus-persisted precedence were undefined (B7.2 left "a new camera whose placement is Level D policy" without an authority path or precedence).
- **Evidence (ER2-1):**
  - `output_connected_with_token_instantiates_and_binds_camera`;
  - `output_disconnected_unbinds_and_preserves_camera`;
  - `persisted_lookup_uses_the_single_authorized_path` (this session's value wins over the persisted entry on reconnect).
- **ADR:** ADR-04.
- **Tests/obligations:** OB-3, OB-19.

### INV-OUTPUT-1 (amended again)
- **Previous (C12.R1 @ `84ce620`):** "Output removal: membership `leave` before the global is destroyed; camera unbound and kept; fullscreen presentation dropped with the INV-PRES-1 configure; interactions orphaned (ownership kept until release, **no camera mutation**); physical pointer relocated at once to the Arrangement-nearest point of a surviving output (half-open); constraints reconciled per INV-INPUT-5 (deferred under a client grab); the removal is a ScaleInteractionEpoch settlement event."
- **Trigger:** PD-F2-18.
- **Replacement:**
  - Output removal enters Core **only through `output_disconnected(&mut OutputLifecycleToken, …)`** (INV-OUTPUT-2). Then:
    - membership `leave` before the global is destroyed;
    - camera unbound and kept, with its value unchanged;
    - fullscreen presentation dropped with the INV-PRES-1 configure;
    - interactions orphaned (ownership kept until release, no camera mutation);
    - physical pointer relocated at once to the Arrangement-nearest point of a surviving output (half-open);
    - constraints reconciled per INV-INPUT-5 (deferred under a client grab);
    - the removal is a ScaleInteractionEpoch settlement event.
- **Why necessary:** the removal path must carry the same authority as creation, since it unbinds cameras.
- **Evidence (ER2-1):** `output_disconnected_unbinds_and_preserves_camera`.
- **ADR:** ADR-04, ADR-07.
- **Tests/obligations:** OB-3, OB-31.

### INV-OUTPUT-2 (new)
- **Trigger:** PD-F2-18.
- **Wording:**
  - **Output lifecycle authority.** Output connection and disconnection, at startup and at runtime hotplug alike, enter Core only through `output_connected(&mut OutputLifecycleToken, OutputDescriptor)` and `output_disconnected(&mut OutputLifecycleToken, OutputId)`.
  - **The token:**
    - is a dedicated authority, distinct from `InputPort`;
    - is minted only by `Core::new`;
    - has private fields (unforgeable), is not Clone, is `!Send` (cannot be stored in protocol state), and is bound to its Core;
    - is held exclusively by the backend's output/display event source.
  - **`OutputDescriptor`** carries hardware facts only: `OutputKey`, mode, scale, refresh. It has no field through which a caller can choose camera position, centre, zoom or binding.
  - **Persisted camera state** is handed to `Core::new` as a passive, read-only lookup table, consulted only inside `output_connected`. No public restore API exists.
- **Why necessary:** a single authorized, coordinate-free path for every output-originated camera write (creation, binding, restore). It also excludes a coordinate backdoor (`set_camera_position` in disguise).
- **Evidence (ER2-1):**
  - p6 (no token) fails with E0061;
  - p7 (forge the token) fails with the private-fields diagnostic;
  - p8 (descriptor with a `center` field) fails with E0560;
  - p9 (`restore_persisted_state`) fails with E0599;
  - `probe_store_output_token_in_state` fails with E0277;
  - tests: connect, disconnect, persisted path, foreign token.
- **ADR:** ADR-08, ADR-04.
- **Tests/obligations:**
  - OB-19, extended: probes 2f, plus a source rule that `infynitae-wayland` never names `OutputLifecycleToken`, `output_connected` or `output_disconnected`.
  - OB-3.

### INV-SCALE-3 (amended again)
- **Previous (C12.R1 @ `84ce620`):** "**ScaleInteractionEpoch.** The camera changes of one logical interaction form one epoch: continuous pan or pinch; a run of discrete pan or zoom steps; a bounded shell navigation stream. During an epoch, decreases are suppressed; increases happen only at rung crossings and only in the epoch's direction (P7 fixes it down); client responses cannot reverse it; individual camera updates are not each a hint change. The epoch ends at an explicit end, at a settlement event (output add/remove/configuration), or after a **settlement timeout** with no further step (for inputs without an explicit end); pointer motion does not end it. At settlement, h is set to the target of the final requirement, evaluated once. The timeout's existence is architectural; its value is provisional (C14.5). Atomic camera actions are one-step epochs."
- **Trigger:** PD-F2-19.
- **Replacement:**
  - **ScaleInteractionEpoch.** The **spatial** changes of one logical interaction form one epoch, for every `InteractionKind ∈ {CameraPan, CameraZoom, WindowMove, WindowResize, ShellStream}`:
    - continuous: pointer or touchpad pan, pinch, interactive window move or resize;
    - a run of discrete pan or zoom steps;
    - a bounded shell stream of navigation or `MoveElement` commands.
  - **During an epoch:** decreases are suppressed; increases happen only at rung crossings and only in the epoch's direction; client responses cannot reverse it; individual updates are not each a hint change.
  - **P7:** a device-bound decrease is applied immediately and fixes the direction to down. It is a correctness rule, not damping.
  - **The epoch ends at:**
    - interaction termination (gesture end, button release, explicit end-of-stream);
    - a settlement event (output add/remove/configuration);
    - for discrete steps without an explicit end, the **settlement timeout** with no further step.

    Pointer motion does not end it.
  - **At settlement,** h is set to the target of the final requirement, evaluated once.
  - The timeout's existence is architectural. Its value is provisional (C14.5; candidate 300 ms; OB-23).
  - Atomic camera or Stage actions are one-step epochs.
- **Why necessary:**
  - The 1R wording grouped camera changes only.
  - B10.4 (`gesture_active` for WindowMove/WindowResize) and D-B8-SCALE.3 had damped Stage-move interactions, and C14.4 SC-E counts Stage movement as a stimulus. Under 1R a window dragged across an output edge renegotiated on every crossing.
- **Evidence (ER2-2):**

  | Case | NEG (1R camera-only rule) | 2R |
  |---|---|---|
  | window drag across B's edge | 39 | **1** |
  | window resize | 39 | 1 (the one change is the P7 correctness decrease) |
  | shell `MoveElement` bursts | 29 | 5 (≤ 2 per epoch, 3 epochs) |
  | camera pan regression | 1 | 1 |
  | shell Navigate regression | 1 | 1 |
- **ADR:** ADR-05.
- **Tests/obligations:** OB-23, OB-28; the T1 port of ER2-2 with the WindowMove negative control (OB-34 context).

### INV-SCALE-4 (amended)
- **Previous (C12.R1 @ `84ce620`):** "(a) No hint change per raw camera event. (b) Changes happen only at rung crossings, at settlement, or by P7. (c) Per epoch: at most rungs crossed + 1 settlement + one P7 forced decrease per non-client stimulus. (d) No oscillation: outside an epoch, client-caused re-evaluations move h only in the direction of the last non-client stimulus (SC-E); inside an epoch, INV-SCALE-3 applies."
- **Trigger:** PD-F2-19.
- **Replacement:**
  - (a) No hint change per raw **spatial** event (camera, Stage-move or resize update).
  - (b), (c) and (d): unchanged.
- **Why necessary:** clause (a) named camera events only.
- **Evidence:** ER2-2.
- **ADR:** ADR-05.

---

**Unchanged by 2R:** every other entry of C12 / C12.R1 (51 entries).

**Totals after 2R:** 57 effective invariants.
- C12 rows still effective unmodified: 39 (the 40 unchanged after 1R, minus INV-CAMERA-3).
- C12.R1 rows still effective unmodified: 12 (16 minus CAMERA-1, OUTPUT-1, SCALE-3, SCALE-4).
- C12.R2 rows: 6 (5 amended, 1 new).
