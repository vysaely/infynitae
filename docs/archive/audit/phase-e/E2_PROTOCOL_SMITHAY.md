# Checkpoint E2 — Wayland Protocol & Smithay 0.7.0 Boundary Audit

**Reviewer:** Independent Principal Systems Architect & Senior Rust/Wayland Compositor Engineer  
**Baseline:** Phase D Final Seal (`c0a1cee310e56b75d32cf30152a09975ff776700`)  
**Pinned Framework:** Smithay 0.7.0 (`index.crates.io-1949cf8c6b5b557f/smithay-0.7.0`)

---

## 1. Wayland Wire Protocol Audit

Every Wayland protocol interaction specified across the state machines (M1, M2, M3, M9, M10, M11, M12, M13, M16, M17) was audited against Wayland core protocol specifications (`wayland.xml`, `xdg-shell.xml`, `pointer-constraints-unstable-v1.xml`, `fractional-scale-v1.xml`, `xdg-activation-v1.xml`):

### 1.1 `xdg_toplevel` Lifecycle & Configure Responses (M1, M2)
- **Initial Mapping Sequence:**
  1. Client: `xdg_wm_base.get_xdg_surface(wl_surface)`
  2. Client: `xdg_surface.get_toplevel()`
  3. Client: `wl_surface.commit()` (without buffer)
  4. Compositor: `xdg_toplevel.configure(size, states)` followed by `xdg_surface.configure(serial)`
  5. Client: `xdg_surface.ack_configure(serial)`
  6. Client: `wl_surface.attach(buffer)`, `wl_surface.commit()`
  - **Audit:** M1 correctly models `AwaitingInitialCommit` -> `AwaitingFirstBuffer` -> `Mapped`. Pre-map buffer commits are detected as client protocol errors (`W-SM-6`), matching the xdg-shell specification.
- **Maximize & Minimize Handling:**
  - `set_maximized` / `unset_maximized` are answered with exactly one configure without the `maximized` state (`INV-PROTO-1`). This satisfies the xdg-shell requirement that compositors must send a configure in response to state change requests.
  - `set_minimized` is safely ignored (allowed by spec).

### 1.2 Popup Lifecycle & Ordered Preemption (M3, M11)
- **Nested Popup Grabs (`W-SM-14`):** When grabbing nested popups, replacing the pointer grab inside Smithay tore down the keyboard grab. The architecture mandates keeping the installed pointer grab (`INV-INPUT-10`), which was verified in harness `xpopup_focus.rs`.
- **Preemption Ordering (`INV-INPUT-10`):**
  When `ActiveElementFocus` changes while popups are active:
  `PopupGrab::ungrab(All)` -> `keyboard.unset_grab` -> `pointer.unset_grab` -> `set_focus(target)`.
  Inspection of Smithay's `src/desktop/wayland/popup/grab.rs` confirms that `ungrab(All)` correctly sends `xdg_popup.popup_done` topmost-first, safely swallowing `Err(DeadResource)` if the client simultaneously destroyed a popup surface.

### 1.3 Pointer Constraints & The Persistent Lock Protocol Violation (M13, Finding E-F2-02)
- **Protocol Spec Requirement:** Under `zwp_pointer_constraints_v1`, `zwp_locked_pointer_v1.unlocked` signals that a lock is no longer active. An `unlocked` event must not be sent if the pointer was not locked.
- **Architectural Policy in `INV-INPUT-5`:**
  *"An explicit `deactivate` is used only when the delivered focus does not change in that batch; otherwise Smithay's `leave()` does it (W-SM-4a)."*
- **Smithay 0.7.0 Reality (`src/wayland/pointer_constraints.rs:143-158`):**
  ```rust
  pub fn deactivate(self) {
      match self.entry.get() {
          PointerConstraint::Confined(confined) => {
              confined.handle.unconfined();
              confined.active.store(false, Ordering::SeqCst);
          }
          PointerConstraint::Locked(locked) => {
              locked.handle.unlocked();
              locked.active.store(false, Ordering::SeqCst);
          }
      }
      if self.lifetime() == WEnum::Value(Lifetime::Oneshot) {
          self.entry.remove_entry();
      }
  }
  ```
- **The Trap:**
  1. For a `Persistent` constraint, calling `deactivate()` emits `unlocked` and sets `active = false`, but **does not remove the entry** from `data.constraints`.
  2. When pointer focus eventually leaves the surface in a subsequent batch (e.g. user focuses another element), Smithay's `pointer.leave()` (`src/wayland/seat/pointer.rs:251-255`) executes:
     ```rust
     with_pointer_constraint(self, &pointer, |constraint| {
         if let Some(constraint) = constraint {
             constraint.deactivate();
         }
     });
     ```
  3. `with_pointer_constraint` retrieves the still-registered persistent constraint and calls `deactivate()` unconditionally!
  4. `locked.handle.unlocked()` is emitted **a second time**, with no intervening `locked` event.
- **Severity: Level A (Blocks Phase F).** Clients (game engines, SDL3, Wine) that maintain strict lock state machines will crash or enter broken states upon receiving duplicate `unlocked` notifications.

---

## 2. Pinned Smithay 0.7.0 Boundary Analysis

### 2.1 The Rotated Output Double-Swap Flaw (Finding E-F2-01)
- **The Architecture Claim (`F-VIEW-1`, `F-RENDER-1`, `PD-F1-04`):**
  In Phase D D7, `PD-F1-04` introduced:
  `- Definition: mode_O is the output mode after the output transform (rotated outputs swap width and height).`
  And in `F-RENDER-1`:
  `OutputDamageTracker::new(mode_O, Scale(k_O), transform_O)`
- **Smithay 0.7.0 Reality (`src/backend/renderer/damage/mod.rs` & `src/backend/renderer/gles/mod.rs`):**
  1. `OutputDamageTracker::render_output()` (lines 330–340):
     ```rust
     let (output_size, output_scale, output_transform) = self.mode.try_into()?;
     let output_transform = output_transform.invert();
     let output_geo = Rectangle::from_size(output_transform.transform_size(output_size));
     ```
     Smithay **expects `output_size` to be the unrotated physical mode** and internally calls `transform_size` to calculate the transformed geometry.
  2. GLES Renderer `render()` (`src/backend/renderer/gles/mod.rs:2056-2068`):
     ```rust
     self.gl.Viewport(0, 0, output_size.w, output_size.h);
     self.gl.Scissor(0, 0, output_size.w, output_size.h);
     if let Transform::_90 | Transform::_270 | ... = transform {
         mem::swap(&mut output_size.w, &mut output_size.h);
     }
     ```
     The GLES renderer sets `glViewport` to the buffer dimensions (`output_size`), and **then swaps width and height itself** for the orthogonal projection matrix!
- **Consequence of Passing Pre-Swapped `mode_O`:**
  - On a 1920×1080 display rotated 90°, `mode_O` is passed as `1080×1920`.
  - Smithay sets `glViewport(0, 0, 1080, 1920)` on a physical 1920×1080 DRM framebuffer—scissoring the right half of the display and rendering out-of-bounds vertically.
  - Smithay swaps `1080×1920` back to `1920×1080` for the projection matrix, inverting the aspect ratio.
  - `damage_tracker` computes `transform_size(1080×1920) = 1920×1080`, applying damage to the wrong coordinates.
- **Correlated Validation Gap:** Every test in `experiments/b8-harness` hardcoded `Transform::Normal`. Not a single test in the repository ever executed a rotated output.
- **Severity: Level A (Blocks Phase F).**

---

## 3. XWayland Satellite Supervisor Model (M19, ADR-10)

The architecture specifies using external `xwayland-satellite` communicating as a standard Wayland client (`INV-XWAYLAND-1`):
1. **Socket Isolation:** The compositor binds `-listenfd` sockets for X11 (verified in ER-6).
2. **Crash & Supervisor Lifecycle:** If satellite exits, all X11 clients are terminated through the standard Wayland client death path (`INV-STAGE-4`).
3. **Restart Handling:** Satellite restart on the same `DISPLAY` is gated under open obligation `OB-33`.
4. **Authority Boundary:** XWayland windows possess no backdoor to Stage placement or Camera manipulation. They are treated strictly as ordinary toplevel elements.
- **Audit Result:** Sound. ADR-10 and M19 isolate X11 cleanly.
