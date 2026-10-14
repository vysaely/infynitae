# KNOW-RES-007: Timeout Detection and Recovery (TDR) and Device Loss Handling

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-007 |
| **title** | Timeout Detection and Recovery (TDR) and Device Loss Handling |
| **subsystem** | Research/MicrosoftGraphics |
| **source_rev** | WDDM 1.0–3.2 / Windows Kernel |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | Compositor resilience and GPU recovery |
| **affected_branches** | N/A (Future Architecture) |
| **related_invariants** | INV-OUTPUT-1, INV-STAGE-3 |
| **adoption_status** | RESEARCH |
| **next_action** | Prototype EGL_CONTEXT_LOST recovery in adapter |

---

## Verified Facts
1. **TDR Mechanism:** When a GPU hardware engine hangs for more than 2 seconds (`TdrDelay`), WDDM resets the hung engine or adapter. DWM intercepts `DXGI_ERROR_DEVICE_RESET`, tears down its rendering device, recreates swapchains, and restores presentation without crashing the desktop session.
2. **Infynitae Decoupled State Advantage:** Infynitae's Core owns all element IDs, anchors, layout, and surface tree snapshots with zero GPU references. If the rendering backend experiences an `EGL_CONTEXT_LOST` event, Core state remains 100% intact.

## Unverified Hypotheses
- **Claim Requiring Validation:** Guaranteed recovery from GPU reset without leaking dmabufs or dropping client sockets must be proven through automated fault injection in Mesa.

## Performance & Correctness Implications
- Prevents desktop compositor crashes when a single gaming client hangs or triggers a GPU hardware fault.

## Limitations & Counterexamples
- Client applications that do not handle context recreation will terminate, but the compositor itself must survive.

## Reproduction Instructions
Review Microsoft Learn:
- *"Timeout detection and recovery (TDR)"* (Microsoft Learn Display Technologies)
- Test GPU recovery via Linux kernel debugfs `amdgpu_gpu_recover`.

## Links & Citations
- Source Documentation: Microsoft Learn WDDM Fault Recovery Guide
- Related Roadmap: [KNOW-ROAD-001: Phase G Hardware DRM Backend](../roadmap/KNOW-ROAD-001-phase-g-hardware-drm-backend.md)
