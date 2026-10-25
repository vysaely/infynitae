# Phase D checkpoint 1R — authority and coordinate matrix deltas

This document records changes against [../D3_AUTHORITY_MATRIX.md](../D3_AUTHORITY_MATRIX.md) and [../D4_COORDINATE_MATRIX.md](../D4_COORDINATE_MATRIX.md) (baseline `e8ec03a`). Rows not listed are unchanged.

## D3 deltas (authority)

| # | State | 1R row | Finding |
|---|---|---|---|
| 10/11 | Camera centre / zoom | Legal writers: `navigation`, each mutation consuming a fresh `Authorized` proof minted only in the port-carrying entry module. **Forbidden:** protocol handlers, client commits (re-fit removed), timers/animation (not in F01), any retained proof (E0277). | PD-F2-01 |
| 13 | `ViewportFit{O}` | **Removed as state.** Replaced by a navigation-history value (the camera before the last fit) used only by an authorized restore. No target, no mode, no isolation. | PD-F2-01, -11 |
| 14 | Navigation animation | **Not in Foundation 01.** A future feature must drive each step through a port-carrying entry. | PD-F2-01 |
| 27 | `PointerModel` | Split: **(a) `Physical{interaction output, OutputLogical p}`** (A, intake; reconstruction: Arrangement-nearest on output loss) and **(b) `Constraint{surface, kind, lock point: SurfaceLocal}`** (A for policy; P for the protocol object). The Canvas lock point is **derived**. | PD-F2-03 |
| 28 | Pointer focus (delivered) | Core decides outside client-owned grab sequences. During them, Smithay's grab decides, and core **uses the delivered focus** for every protocol-order decision (W-SM-4a). Documented as subordinate routing, not a second authority. | PD-F2-02 |
| 30 | Keyboard focus | Renamed **ActiveElementFocus** (element-level). Single writer: FocusModel. **Smithay popup grabs no longer listed as writers**: they are ProtocolSurfaceRoute (row 30b), preempted by the INV-INPUT-10 order. | PD-F2-02 |
| 30b (new) | ProtocolSurfaceRoute (popup grab chain) | Class P (adapter). Owner: adapter (Smithay `PopupGrab`). Legal writers: validated client `xdg_popup.grab` (root = focused element), client popup destruction, adapter preemption. Readers: Smithay seat. Lifetime: grab chain. Forbidden: changing the element-level focus. Reconstruction: none needed (protocol). | PD-F2-02 |
| 22 | `ScaleState` | Adds `epoch{open, kind, direction, deadline}`. The deadline timer is armed only while the epoch is open (INV-IDLE-1). | PD-F2-07 |
| 26 | `FullscreenState` | No pending/deferred request state (decline at once). | PD-F2-10 |
| 36 | `D_max` | Re-queried on renderer recreation (PD-F1-08, now normative in INV-SCALE-2) | PD-F2-08 |

**Authority searches re-run on the 1R matrix:**

| Search | Result |
|---|---|
| S1 multiple writers | Camera: one path (proof-gated). Keyboard focus: one element-level writer, with routing subordinate and preempted. Pointer focus: core, or Smithay during a client sequence; core uses the delivered focus, so there is no conflict. **None open.** |
| S2 authority cycles | The client-size → re-fit → camera cycle is **removed** (ER-4, ER-5 R3). The h ↔ client size loop is bounded by P7 + epoch + SC-E (ER-5 R2). **None open.** |
| S3 unreconstructable derived state | The lock point is now surface-relative (reconstructable). The "callback pending" datum remains D-F1 PD-F1-06 (Phase F snapshot field). |
| S4 caches without invalidation | Unchanged D-F1 items (PD-F1-03/04/07/08). The ladder is recomputed on hotplug; off-ladder held values are allowed by INV-SCALE-2. |
| S5 protocol objects outliving authority | Constraint under a client grab: deferred **consistently** (ER-2). Popup grab outliving a focus decision: preempted (ER-1). Deferred fullscreen request: removed. **None open.** |
| S6 shell/backend durable truth | The animation owner is removed from Foundation 01. **None.** |

## D4 deltas (coordinates)

| # | Conversion | 1R formula | Rounding | Finding / evidence |
|---|---|---|---|---|
| X17 | pan Σ → camera | **`C = C_start − Q16(Σ dp_phys / (s_O · z_C))`**, where `dp_phys = dp_logical · s_O` on the interaction output; f64 sum from the gesture origin; per-event terms when s or z change within the gesture | R2 (once per update) | PD-F2-06; ER-5 R1 (≤ 0.9 px of the hand at zoom 0.5…2.5, scale 1.0/1.5) |
| X20 | popup constraint target | viewport via X8 → parent-geometry-relative → **R8: ceiling at the minimum edges, floor at the maximum edges** → `Rectangle<i32>` | **R8 (new)** | PD-F2-12 |
| X25 | lock point | stored `SurfaceLocal` (f64, Q16-exact); Canvas = `origin(S) + at` | none | PD-F2-03 |
| X26 | relocation on output loss | Arrangement-nearest, half-open clamp `≤ w − 2⁻⁸` (PD-F1-05 adopted) | clamp | PD-F2-03; ER-2 |
| X27 (new) | confinement clamp | `p := clamp_halfopen(p + Δ, project_O(region) ∩ [0, logical_O))` on the interaction output; no Arrangement crossing | clamp | PD-F2-05; ER-5 R5 |
| X21 | fullscreen presentation view | unchanged formula (PD-F1-09); starts at grant | R7 | PD-F2-10 |

**Re-falsification rows changed:**
- **Gesture-total accumulation:** now holds at every zoom (ER-5 R1). The sealed formula is kept as a negative control.
- **Reference-relative lattice:** render/hit agreement holds (ED-1). Shape jitter during zoomed panning remains a stated residual (OB-24 rescoped).
- **Multi-output inverse:** the `Locked` output gap is closed (PD-F2-03); confinement crossing is closed (PD-F2-05); drag across outputs is OB-34.

**Rounding never leaks back into Stage geometry (re-check).** R8 produces a positioner target, which is protocol input and never Stage. X17 writes the camera from device Σ. The derived lock point is never stored. **Holds.**

> **2R note:** rows 10/11/12 (camera centre/zoom/binding) and the camera-lifecycle authority are further amended in [../2R/MATRICES_2R.md](../2R/MATRICES_2R.md) (PD-F2-18). Row 22 (`ScaleState` epoch) is amended there for PD-F2-19.
