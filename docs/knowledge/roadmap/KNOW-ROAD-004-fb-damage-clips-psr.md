# KNOW-ROAD-004: DRM Damage Clips and Panel Self Refresh (PSR) Optimization

| Field | Value |
| :--- | :--- |
| **id** | KNOW-ROAD-004 |
| **title** | DRM Damage Clips and Panel Self Refresh (PSR) Optimization |
| **subsystem** | Roadmap/PowerOptimization |
| **source_rev** | Linux DRM FB_DAMAGE_CLIPS |
| **date** | 2026-10-14 |
| **evidence_class** | HYPOTHESIS |
| **applicability** | Future Phase G native DRM backend |
| **affected_branches** | N/A (Phase G Milestone) |
| **related_invariants** | INV-CAMERA-4 |
| **adoption_status** | RESEARCH |
| **next_action** | Serialize Core damage rects to FB_DAMAGE_CLIPS blob property |

---

## Verified Facts
1. Modern laptop displays (eDP) support Panel Self Refresh (PSR / PSR2), allowing the display panel to refresh from its own internal frame buffer while the GPU and display engine enter low-power sleep states.
2. The Linux DRM atomic API accepts the `FB_DAMAGE_CLIPS` blob property, informing the hardware which scan lines were modified.

## Unverified Hypotheses
- **Hypothesis Requiring Validation:** Propagating Core damage rects directly to `FB_DAMAGE_CLIPS` increases PSR residency from 0% to $>85\%$ during typical typing/terminal usage, reducing system power draw by $>1.2\,\text{W}$.

## Performance & Correctness Implications
- Saves memory bus bandwidth and battery life on portable devices.

## Limitations & Counterexamples
- Some older eDP panel controllers exhibit visual glitching or line tearing under PSR2 selective update; driver blacklisting must be respected.

## Reproduction Instructions
Monitor PSR residency via `/sys/kernel/debug/dri/0/eDP-1/psr_status`.

## Links & Citations
- Related Research: [KNOW-RES-004: DirectFlip, Independent Flip, and MPO](../research/KNOW-RES-004-msft-directflip-independent-flip-mpo.md)
- Related Roadmap: [KNOW-ROAD-001: Phase G Hardware DRM Backend](KNOW-ROAD-001-phase-g-hardware-drm-backend.md)
