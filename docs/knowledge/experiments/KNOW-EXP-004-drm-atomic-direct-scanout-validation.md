# KNOW-EXP-004: DRM Atomic Direct Scanout Validation Protocol

| Field | Value |
| :--- | :--- |
| **id** | KNOW-EXP-004 |
| **title** | DRM Atomic Direct Scanout Validation Protocol |
| **subsystem** | Experiments/DirectScanout |
| **source_rev** | Phase G Blueprint |
| **date** | 2026-10-14 |
| **evidence_class** | HYPOTHESIS |
| **applicability** | Future Phase G native DRM backend |
| **affected_branches** | N/A (Phase G Milestone) |
| **related_invariants** | INV-PRES-1 |
| **adoption_status** | PROPOSED |
| **next_action** | Implement test harness in infynitae-backend-headless with mock DRM planes |

---

## Verified Facts
1. Windows DirectFlip and Independent Flip prove that direct scanout eliminates composition passes, reducing display latency to the physical scanout floor.
2. In Linux DRM/KMS, direct scanout requires setting the client's `FB_ID` directly on the primary or overlay plane during an atomic commit (`ALLOW_PRIMARY_PLANE_SCANOUT_ANY`).

## Unverified Hypotheses
- **Hypothesis Requiring Validation:** Direct scanout will achieve a statistically significant ($>4\,\text{ms}$) latency reduction on high-refresh displays without increasing page-flip timing jitter or causing compositor stall bubbles when overlays appear.

## Performance & Correctness Implications
- Saves GPU rasterization and composition memory bandwidth.
- Requires atomic plane configuration validation via `DRM_MODE_ATOMIC_TEST_ONLY`.

## Limitations & Counterexamples
- Cannot scan out buffers whose DRM modifiers or pixel formats are rejected by the display controller hardware.

## Reproduction Instructions
Execute the proposed DRM atomic scanout test protocol:
1. Connect a headless virtual output with simulated DRM planes.
2. Commit a fullscreen dmabuf buffer.
3. Assert that `render_frame()` is bypassed and presentation feedback records `KIND_HW_COMPLETION`.

## Links & Citations
- Related Research: [KNOW-RES-004: DirectFlip, Independent Flip, and MPO](../research/KNOW-RES-004-msft-directflip-independent-flip-mpo.md)
- Related Roadmap: [KNOW-ROAD-001: Phase G Hardware DRM Backend](../roadmap/KNOW-ROAD-001-phase-g-hardware-drm-backend.md)
