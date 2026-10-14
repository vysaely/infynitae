# KNOW-ROAD-001: Phase G Hardware DRM/KMS Backend Blueprint

| Field | Value |
| :--- | :--- |
| **id** | KNOW-ROAD-001 |
| **title** | Phase G Hardware DRM/KMS Backend Blueprint |
| **subsystem** | Roadmap/BackendDRM |
| **source_rev** | Architecture Milestone Plan |
| **date** | 2026-10-14 |
| **evidence_class** | HYPOTHESIS |
| **applicability** | Future Phase G milestone |
| **affected_branches** | N/A (Upcoming Increment) |
| **related_invariants** | INV-ARCH-3, INV-PRES-1 |
| **adoption_status** | PROPOSED |
| **next_action** | Draft Phase G architectural specification |

---

## Verified Facts
1. Infynitae currently runs on headless virtual outputs and nested Winit. Bare-metal execution on physical display hardware requires a dedicated DRM/KMS backend (`infynitae-backend-udev`).
2. The hardware backend must support:
   - Atomic modesetting (`drmModeAtomicCommit`).
   - Direct scanout on primary and overlay planes for fullscreen presentations (`INV-PRES-1`).
   - Hardware cursor plane management with software composition fallback.
   - High-priority rendering context (`EGL_CONTEXT_PRIORITY_HIGH_IMG`).
   - GPU reset recovery (`EGL_CONTEXT_LOST`).

## Unverified Hypotheses
- **Hypothesis Requiring Validation:** Direct scanout will achieve a statistically significant ($>4\,\text{ms}$) latency reduction on high-refresh displays without increasing page-flip timing jitter.

## Performance & Correctness Implications
- Delivers native display refresh rates, zero tearing, minimal latency, and full GPU hardware acceleration.

## Limitations & Counterexamples
- Requires `libseat` or `logind` session management for VT switching and device permission handling without root privileges.

## Reproduction Instructions
Execute the Phase G test harness blueprint when implemented in `infynitae-backend-udev`.

## Links & Citations
- Related Research: [KNOW-RES-004: DirectFlip, Independent Flip, and MPO](../research/KNOW-RES-004-msft-directflip-independent-flip-mpo.md)
- Related Research: [KNOW-RES-007: TDR and Device Loss Recovery](../research/KNOW-RES-007-msft-tdr-device-loss-recovery.md)
