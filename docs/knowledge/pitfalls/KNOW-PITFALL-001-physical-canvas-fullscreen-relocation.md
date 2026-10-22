# KNOW-PITFALL-001: Physical Canvas Fullscreen Relocation

| Field | Value |
| :--- | :--- |
| **id** | KNOW-PITFALL-001 |
| **title** | Physical Canvas Fullscreen Relocation |
| **subsystem** | Pitfalls/Presentation |
| **source_rev** | DriftWM / Infynitae INC-7 Review |
| **date** | 2026-10-14 |
| **evidence_class** | CONTRADICTED |
| **applicability** | Fullscreen layout architecture |
| **affected_branches** | N/A (Rejected Anti-Pattern) |
| **related_invariants** | INV-PRES-1, INV-CAMERA-4 |
| **adoption_status** | REJECTED |
| **next_action** | Maintain camera-independent projection view |

---

## Verified Facts
1. Upstream DriftWM implements fullscreen by physically relocating the window in infinite canvas space to the output's camera origin (`camera_i32`) and forcing zoom to 1.0.
2. When the window exits fullscreen, DriftWM must execute complex reflow heuristics (`pending_recenter`, `establish_exit_placement`). If a client commits back at its pre-exit size or changes sizes mid-settle, window positioning desynchronizes or corrupts neighboring snapped windows (`dev/docs/caveats.md:113`).
3. Infynitae formally rejected physical relocation in favor of `INV-PRES-1` (pure camera-independent presentation projection).

## Unverified Hypotheses
- None; the failure modes of physical relocation are documented in DriftWM caveats and configure tests.

## Performance & Correctness Implications
- Physical relocation causes coordinate desynchronization, spurious layout reflow, and displacement bugs.

## Limitations & Counterexamples
- Reconsideration would only be justified if an underlying graphics hardware architecture physically required coordinate origin translation on scanout memory (no known hardware requires this).

## Reproduction Instructions
Review DriftWM caveat documentation on `pending_recenter`:
```bash
view_file /home/<user>/.gemini/antigravity/scratch/driftwm/dev/docs/caveats.md (lines 110-120)
```

## Links & Citations
- Related Architecture: [KNOW-ARCH-001: Camera-Independent Fullscreen Presentation Projection](../architecture/KNOW-ARCH-001-camera-projection-independence.md)
- Related Research: [KNOW-RES-002: DriftWM Fullscreen Lifecycle](../research/KNOW-RES-002-driftwm-fullscreen-lifecycle.md)
