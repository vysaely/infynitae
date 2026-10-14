# KNOW-ROAD-003: Variable Refresh Rate (VRR) and Adaptive Frame Pacing

| Field | Value |
| :--- | :--- |
| **id** | KNOW-ROAD-003 |
| **title** | Variable Refresh Rate (VRR) and Adaptive Frame Pacing |
| **subsystem** | Roadmap/FramePacing |
| **source_rev** | Linux DRM VRR / Wayland Tearing |
| **date** | 2026-10-14 |
| **evidence_class** | HYPOTHESIS |
| **applicability** | Future Phase G native DRM backend |
| **affected_branches** | N/A (Phase G Milestone) |
| **related_invariants** | INV-PRES-1 |
| **adoption_status** | RESEARCH |
| **next_action** | Prototype VRR_ENABLED connector query in DRM backend |

---

## Verified Facts
1. Displays supporting Adaptive-Sync / FreeSync / G-Sync can dynamically adjust vblank intervals to match client render rate.
2. In Linux DRM, VRR is activated via the `VRR_ENABLED` property on the CRTC.
3. Wayland supports asynchronous page flips and tearing via `wp_tearing_control_v1`.

## Unverified Hypotheses
- **Hypothesis Requiring Validation:** VRR activation on fullscreen presentations does not introduce display panel flicker when frame rates fluctuate rapidly between 40 Hz and 144 Hz.

## Performance & Correctness Implications
- Eliminates stutter and micro-judder in gaming workloads when frame rates deviate from the monitor's native refresh rate.

## Limitations & Counterexamples
- VRR must be strictly disabled on outputs displaying mixed desktop UI or video playback to avoid panel brightness fluctuation (LFC flicker).

## Reproduction Instructions
Inspect DRM connector properties via `drm_info` on a VRR-capable display.

## Links & Citations
- Related Research: [KNOW-RES-005: DXGI Waitable Swapchains](../research/KNOW-RES-005-msft-dxgi-waitable-frame-pacing.md)
- Related Roadmap: [KNOW-ROAD-001: Phase G Hardware DRM Backend](KNOW-ROAD-001-phase-g-hardware-drm-backend.md)
