# KNOW-RES-004: Microsoft DirectFlip, Independent Flip, and Multi-Plane Overlays

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-004 |
| **title** | Microsoft DirectFlip, Independent Flip, and Multi-Plane Overlays |
| **subsystem** | Research/MicrosoftGraphics |
| **source_rev** | MSDN / WDDM DDK 2.x |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | Future Phase G native DRM/KMS backend |
| **affected_branches** | N/A (Future Architecture) |
| **related_invariants** | INV-PRES-1, F-PRESENT-1 |
| **adoption_status** | RESEARCH |
| **next_action** | Prototype atomic overlay plane assignment in Phase G |

---

## Verified Facts
> **Reconciliation 2026-10-14 (main `f323c2d`):** the Microsoft statements below were not checked against Microsoft documentation in any session, and some are interpretations ("exact parity", magnitudes). Read them with the MS-DOC / INTERP / APPLY / WIN-ONLY classes of [KNOW-RES-008](KNOW-RES-008-msft-research-claim-classification.md).

1. **DirectFlip:** In Windows 8+, when a DXGI swapchain matches display dimensions and format with no overlapping translucent surfaces, DWM bypasses the composition blit and swaps the application buffer allocation directly to the primary scanout plane.
2. **Independent Flip (IFlip):** In Windows 10+, DWM suspends its composition thread entirely; the application swapchain presents directly to display hardware via `dxgkrnl`, delivering Fullscreen Exclusive (FSE) latency while preserving windowed desktop semantics.
3. **Multi-Plane Overlays (MPO):** Hardware display planes allow IFlip to remain active for a game/video surface while desktop UI, notification toasts, or subtitles are scanned out simultaneously on separate hardware planes.
4. **Linux Equivalence:** Linux DRM/KMS atomic planes (`DRM_PLANE_TYPE_PRIMARY`, `DRM_PLANE_TYPE_OVERLAY`) provide exact architectural parity via `drmModeAtomicCommit`.

## Unverified Hypotheses
- **Claim Requiring Validation:** Guaranteed latency and power savings from direct scanout must be empirically measured on Linux DRM drivers (AMD, Intel, NVIDIA) to quantify real-world frame pacing jitter versus GLES composition.

## Performance & Correctness Implications
- Bypasses composition GPU passes, eliminating $4\text{–}16\,\text{ms}$ of composition latency and saving memory bandwidth.
- Requires atomic plane configuration validation (`DRM_MODE_ATOMIC_TEST_ONLY`) before commitment.

## Limitations & Counterexamples
- Cannot be engaged if client buffer pixel format or modifier (`drm_fourcc`) is incompatible with the display plane, or if hardware cursor scanout is unsupported.

## Reproduction Instructions
Inspect Microsoft Learn documentation on DXGI Flip Model and WDDM MPO:
- *"DXGI Flip Model, DirectFlip, and Independent Flip"* (Microsoft Learn)
- Reference test plan in [KNOW-EXP-004: DRM Atomic Direct Scanout Validation](../experiments/KNOW-EXP-004-drm-atomic-direct-scanout-validation.md)

## Links & Citations
- Source Documentation: Microsoft Learn Windows Client Development
- Related Roadmap: [KNOW-ROAD-001: Phase G Hardware DRM Backend](../roadmap/KNOW-ROAD-001-phase-g-hardware-drm-backend.md)
- Related Architecture: [KNOW-ARCH-001: Camera-Independent Fullscreen Presentation Projection](../architecture/KNOW-ARCH-001-camera-projection-independence.md)
