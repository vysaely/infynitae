# KNOW-RES-006: WDDM Monitored Fences and Timeline Synchronization

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-006 |
| **title** | WDDM Monitored Fences and Timeline Synchronization |
| **subsystem** | Research/MicrosoftGraphics |
| **source_rev** | WDDM 2.0 / D3D12 SDK |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | GPU synchronization architecture |
| **affected_branches** | N/A (Future Architecture) |
| **related_invariants** | INV-ARCH-3 |
| **adoption_status** | RESEARCH |
| **next_action** | Track linux-drm-syncobj-v1 support in Wayland adapter |

---

## Verified Facts
1. **Monitored Fences:** WDDM 2.0 introduced 64-bit monotonically increasing integer timeline fences (`ID3D12Fence`), shared cross-process between applications, the DWM, and display hardware without kernel transitions.
2. **Implicit Sync Flaws:** Legacy Linux graphics relied on implicit synchronization (`dma-fence`), which couples buffer readiness to kernel memory allocations, risking priority inversion, driver deadlocks, and unpredictable pipeline stalls.
3. **Linux Timeline Syncobjs:** The Linux DRM subsystem introduced `drm_syncobj` timeline points, and Wayland merged `linux-drm-syncobj-v1`, establishing exact parity with WDDM monitored fences.

## Unverified Hypotheses
- **Claim Requiring Validation:** Mandatory replacement of implicit synchronization with explicit syncobjs must be verified across legacy drivers to ensure backward compatibility fallback paths.

## Performance & Correctness Implications
- Eliminates CPU stall bubbles in the compositor render loop.
- Enables Vulkan-grade fine-grained submission tracking.

## Limitations & Counterexamples
- Requires kernel $\ge 6.8$ and Mesa $\ge 24.1$ for complete Wayland `linux-drm-syncobj-v1` client and compositor integration.

## Reproduction Instructions
Review Microsoft Learn:
- *"Monitored fence synchronization"* (WDDM 2.0 Specification)
- Inspect Linux kernel `drm_syncobj.c` and Wayland `linux-drm-syncobj-v1.xml`.

## Links & Citations
- Source Documentation: Microsoft Learn Windows Display Driver Model
- Related Roadmap: [KNOW-ROAD-002: Explicit Timeline Sync with Syncobj](../roadmap/KNOW-ROAD-002-explicit-timeline-sync-syncobj.md)
