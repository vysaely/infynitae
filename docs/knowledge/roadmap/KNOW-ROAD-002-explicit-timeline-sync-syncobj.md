# KNOW-ROAD-002: Explicit Timeline Synchronization with linux-drm-syncobj-v1

| Field | Value |
| :--- | :--- |
| **id** | KNOW-ROAD-002 |
| **title** | Explicit Timeline Synchronization with linux-drm-syncobj-v1 |
| **subsystem** | Roadmap/Synchronization |
| **source_rev** | Wayland Protocols 2024 / Linux DRM 6.8+ |
| **date** | 2026-10-14 |
| **evidence_class** | HYPOTHESIS |
| **applicability** | Future Phase G native DRM backend |
| **affected_branches** | N/A (Phase G Milestone) |
| **related_invariants** | INV-ARCH-3 |
| **adoption_status** | PROPOSED |
| **next_action** | Implement linux-drm-syncobj-v1 protocol handler in infynitae-wayland |

---

## Verified Facts
1. Explicit synchronization using 64-bit monotonically increasing timeline points (`linux-drm-syncobj-v1`) replaces implicit dma-fence kernel synchronization.
2. In explicit sync, clients pass an acquire syncobj timeline point with their buffer commit; the compositor passes this fence directly to the DRM atomic commit (`IN_FENCE_FD`), allowing the display controller to wait for GPU completion directly without CPU blockage.

## Unverified Hypotheses
- **Hypothesis Requiring Validation:** Explicit syncobj eliminates 100% of GPU driver pipeline stalls under Vulkan rendering workloads on modern Mesa drivers.

## Performance & Correctness Implications
- Eliminates CPU stall bubbles in the compositor render loop and prevents priority inversion deadlocks.

## Limitations & Counterexamples
- Requires kernel $\ge 6.8$ and Mesa $\ge 24.1$; a fallback to implicit sync must be maintained for legacy clients.

## Reproduction Instructions
Inspect `linux-drm-syncobj-v1.xml` in `wayland-protocols`.

## Links & Citations
- Related Research: [KNOW-RES-006: WDDM Monitored Fences and Timeline Sync](../research/KNOW-RES-006-msft-wddm-fences-timeline-sync.md)
- Related Roadmap: [KNOW-ROAD-001: Phase G Hardware DRM Backend](KNOW-ROAD-001-phase-g-hardware-drm-backend.md)
