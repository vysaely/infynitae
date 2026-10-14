# KNOW-ARCH-001: Camera-Independent Fullscreen Presentation Projection

| Field | Value |
| :--- | :--- |
| **id** | KNOW-ARCH-001 |
| **title** | Camera-Independent Fullscreen Presentation Projection |
| **subsystem** | Core/Presentation |
| **source_rev** | ccb82c5 / 489180e |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | All Phase F / INC-7+ branches |
| **affected_branches** | main, hardening/inc-6-scale-policy, experimental/inc7-exit-readiness-2026-10-14 |
| **related_invariants** | INV-PRES-1, INV-CAMERA-4, F-PRESENT-1 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | Maintain test coverage in t1_fullscreen |

---

## Verified Facts
1. In Infynitae Native, when a mapped toplevel enters fullscreen (`SetFullscreen`), its canvas anchor and layout position are **never mutated** (`INV-PRES-1`, `INV-CAMERA-4`).
2. Fullscreen presentation is implemented purely by the projection pipeline overriding the output viewport: the presenting element is projected directly to output bounds at scale 1.0, while background camera panning continues without moving the window's anchor.
3. This completely prevents the coordinate corruption, window displacement, and complex recentering logic (`pending_recenter`) observed in traditional compositors like DriftWM.

## Unverified Hypotheses
- Hypothesized that multi-output canvas panning under fullscreen presentation may benefit from hardware overlay plane direct scanout without any performance degradation.

## Performance & Correctness Implications
- Eliminates $\mathcal{O}(N)$ canvas reflow on fullscreen toggle.
- Guarantees zero spurious damage propagation to fullscreen outputs during canvas navigation.
- Preserves exact restore geometry without relying on heuristic size-tracking caches.

## Limitations & Counterexamples
- An occluded window hidden behind a fullscreen presentation must not receive keyboard focus unless explicitly presented on another output (see `KNOW-DEC-002`).

## Reproduction Instructions
Execute Core presentation invariant tests:
```bash
cargo test -p infynitae-core --lib a_presentation_is_independent_of_the_camera
```

## Links & Citations
- Upstream Invariant: `FOUNDATION_INVARIANTS.md` (`INV-PRES-1`)
- Related Pitfall: [KNOW-PITFALL-001: Physical Canvas Fullscreen Relocation](../pitfalls/KNOW-PITFALL-001-physical-canvas-fullscreen-relocation.md)
- Related Decision: [KNOW-DEC-001: Pre-Map Fullscreen Ownership](../decisions/KNOW-DEC-001-a3-premap-fullscreen-ownership.md)
