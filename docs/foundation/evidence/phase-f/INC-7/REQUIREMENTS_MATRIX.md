# INC-7 Requirements Traceability Matrix & Architectural Plan

**Status:** PLANNING RECORD & REQUIREMENTS RECOVERY (PRE-IMPLEMENTATION)  
**Milestone:** INC-7 (Product / Headless)  
**Category:** PRODUCT  
**Execution Class:** HEADLESS  
**Prerequisites:** INC-4 ratified (ActiveElementFocus available)  
**Objective:** Toplevel request policy: fullscreen, maximize, `activated` (M2, M9, INV-PRES-1, INV-PROTO-1, INV-CAMERA-4).

---

## 1. Requirements Traceability Matrix

| Requirement | Canonical Spec Ref | Planned Architectural Location | Planned Verification / Test Target |
|---|---|---|---|
| **OBJ-19** | `FOUNDATION_OBJECT_MODEL.md` § OBJ-19 | `infynitae-core/src/protocol.rs::{FullscreenState, OutputPresentation}` | T0 unit tests in Core; T1 `t1_fullscreen.rs` |
| **F-PRESENT-1** | `FOUNDATION_ARCHITECTURE.md` § F-PRESENT-1 | `infynitae-core/src/projection.rs` & `render.rs` | T0 presentation view centering & cropping; T1 render parity |
| **INV-PRES-1** | `FOUNDATION_INVARIANTS.md` § INV-PRES-1 | `infynitae-core/src/protocol.rs` | T0 grant/decline policy; T1 wire configure checks |
| **INV-PROTO-1** | `FOUNDATION_INVARIANTS.md` § INV-PROTO-1 | `infynitae-core/src/protocol.rs`, `infynitae-wayland` | Exactly one configure per request; maximize declined |
| **INV-CAMERA-4** | `FOUNDATION_INVARIANTS.md` § INV-CAMERA-4 | `infynitae-core/src/protocol.rs` | Fullscreen never touches camera center, zoom, or anchor |
| **M2** | `FOUNDATION_STATE_MACHINES.md` § M2 | `infynitae-core/src/protocol.rs` | T0 request transition tests; negative control |
| **M9** | `FOUNDATION_STATE_MACHINES.md` § M9 | `infynitae-core/src/protocol.rs` | T0 fullscreen replacement, exit on unmap/disconnect |
| **OB-32** | `FOUNDATION_TEST_PLAN.md` § OB-32 | `infynitae-backend-headless/tests/t1_fullscreen.rs` | TST-22 `xmaximize` production port (maximize and fullscreen answers) |
| **OB-22** | `FOUNDATION_TEST_PLAN.md` § OB-22 | `infynitae-backend-headless/tests/t1_fullscreen.rs` | T1 fullscreen configure wire sequence & focus independence |
| **OB-18** | `FOUNDATION_TEST_PLAN.md` § OB-18 | `infynitae-core/src/protocol.rs` | Popup behavior with fullscreen parents |
| **W-SM-13** | `FOUNDATION_ARCHITECTURE.md` § W-SM-13 | `infynitae-wayland/src/handlers.rs` | Pre-configure request overrides |
| **Negative Controls** | `FOUNDATION_TEST_PLAN.md` § INC-7 | `tools/verify-test-efficacy` | Record-only / deferral (0 answers) must fail |

---

## 2. Architectural Design Details

### 2.1 Core Fullscreen State (`OBJ-19`)
- Core owns `FullscreenState`:
  - `active: Vec<(OutputId, FullscreenRecord)>`
  - `FullscreenRecord { element: ElementId, saved_size: Option<LocalSize>, output: OutputId }`
- Authority:
  - Only client requests routed through `protocol.rs` mutate `FullscreenState`.
  - Navigation, camera, and stage code are strictly forbidden from mutating `FullscreenState`.
  - When granted, the presentation starts (F-PRESENT-1).

### 2.2 Decision Rules for `set_fullscreen` (INV-PRES-1, M9)
1. **Grant Precondition:** Granted ONLY IF:
   - Window holds `ActiveElementFocus`, OR
   - Window is in pre-map state (`Lifecycle::AwaitingInitialCommit` or `Lifecycle::AwaitingFirstBuffer`).
2. **Decline Policy:** If not focused and already mapped, request is declined immediately:
   - Emits exactly one `Configure` without `fullscreen` state.
   - Nothing stored in `FullscreenState`.
3. **Output Resolution:**
   - If output parameter specified by client is valid, use requested output.
   - Otherwise, select the output currently observing the element (highest area or lowest OutputId if tied).
4. **Replacement Policy:**
   - If output already hosts a fullscreen element $W_{\text{old}}$, $W_{\text{old}}$ is exited and receives configure with its `saved_size`.
   - New element $W_{\text{new}}$ becomes fullscreen on that output.
5. **Exit Triggers:**
   - `unset_fullscreen` request from client.
   - Window unmapped or destroyed.
   - Output hosting fullscreen presentation disconnected.
   - All exit triggers emit configure with saved size.
   - Focus loss does **NOT** end fullscreen.

### 2.3 Maximize & Minimize Policy (INV-PROTO-1, M2)
- Infynitae is a spatial canvas compositor without tiling or desktop-bound maximize.
- `set_maximized` / `unset_maximized`:
  - Answered with **exactly one configure** that does **NOT** contain `XdgToplevelState::Maximized`.
  - Stage anchor, size, camera, and window position remain completely unchanged.
- `set_minimized`:
  - Ignored without error (no configure, no mutation).

### 2.4 Presentation View Geometry (`F-PRESENT-1`)
- For output $O$ and fullscreen window $W$:
  - Presentation view is evaluated at presentation zoom $1.0$.
  - View calculation:
    $$v_P = \text{anchor}(W) - \max\left(0, \frac{\text{view}_O / \text{scale}_O - \text{size}(W)}{2}\right)$$
  - Windows smaller than output extent are centered with letterbox/pillarbox borders.
  - Windows larger than output extent are origin-aligned and cropped.
  - Other cameras observing canvas see $W$ at its real canvas position without alteration.

---

## 3. Implementation Plan & Work Isolation

1. **Branch Isolation:**
   - Branch: `dev/inc-7-toplevel-requests`.
   - Based on canonical `main` (`db254c24bfbb6c5598fdb5c7bf4fd79ba12f4524`).
   - Does NOT assume INC-6 is ratified; completely decoupled and independent.
2. **Verification Requirements:**
   - Port TST-22 (`xmaximize`) into `t1_fullscreen.rs`.
   - Verify all 14 architectural source rules.
   - Pass `./tools/verify-architecture` and `./tools/verify-test-efficacy`.
