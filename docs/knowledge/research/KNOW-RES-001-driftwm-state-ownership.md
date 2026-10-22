# KNOW-RES-001: DriftWM State Ownership and Space Bypass

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-001 |
| **title** | DriftWM State Ownership and Space Bypass |
| **subsystem** | Research/DriftWM |
| **source_rev** | 352333a8fa1b22171492d4b71a54102045c9a19d |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | Architectural comparison with Smithay desktop abstractions |
| **affected_branches** | N/A (External Reference) |
| **related_invariants** | INV-STAGE-1, INV-ARCH-3 |
| **adoption_status** | RESEARCH |
| **next_action** | Maintain clean-room Core authority separation |

---

## Verified Facts
1. Upstream DriftWM bypasses Smithay's built-in `Space` element management for window layout and z-order. It introduces its own `Stage<W>` struct ([`src/stage/mod.rs`](file:///home/<user>/.gemini/antigravity/scratch/driftwm/src/stage/mod.rs)), enforcing via Clippy lints that `Space` must never mutate window positions.
2. However, DriftWM still relies on Smithay's `Window` struct to store toplevel state, popup maps, and subsurfaces, resulting in dual state ownership between `Stage` and Smithay `Window`.
3. Infynitae Native achieves a far more rigorous separation: `infynitae-core` owns all authoritative layout, z-order, element lifecycle, and surface tree snapshots with zero dependencies on Smithay or Wayland server types.

## Unverified Hypotheses
- Hypothesized that DriftWM's `Stage` could be simplified to pure integer coordinates without breaking snap reflow; not verified in DriftWM.

## Performance & Correctness Implications
- Infynitae's clean Core-Adapter boundary guarantees that Core state can be formally verified via mathematical proofs and state-machine tests without running an event loop or mocking Wayland displays.

## Limitations & Counterexamples
- DriftWM is licensed under **GPL-3.0-or-later**. Zero lines of code or algorithm implementations may be copied into Infynitae Native.

## Reproduction Instructions
Inspect DriftWM's `Stage` implementation:
```bash
view_file /home/<user>/.gemini/antigravity/scratch/driftwm/src/stage/mod.rs
```

## Links & Citations
- Upstream Repository: `https://github.com/malbiruk/driftwm`
- Independent Review: `INDEPENDENT_REVIEW.md` (archived in `~/Infynitae-KNOWLEDGE-SOURCES-2026-10-14/agy-scratch/`); benchmark reproduction: [KNOW-EXP-001](../experiments/KNOW-EXP-001-pull-cost-bench-reproduction.md)
- Camera, viewport and hit-testing: [KNOW-RES-009](KNOW-RES-009-driftwm-camera-viewport-and-hit-testing.md)
