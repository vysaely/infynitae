# KNOW-DEC-001: Pre-Map Fullscreen Request Ownership and Deferred Eviction (Finding A-3)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-DEC-001 |
| **title** | Pre-Map Fullscreen Request Ownership and Deferred Eviction (Finding A-3) |
| **subsystem** | Core/Protocol |
| **source_rev** | 07acfeb |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | Fullscreen protocol lifecycle |
| **affected_branches** | experimental/inc7-exit-readiness-2026-10-14 |
| **related_invariants** | INV-PRES-1, C12.F5 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | Formally ratify C12.F5 amendment in governance |

---

## Verified Facts
1. **The Dilemma (Finding A-3):** As originally sealed in `INV-PRES-1`, fullscreen was granted before a window's first map, and a newer grant replaced an older one on the same output immediately. Consequently, an unmapped newcomer from any client evicted the currently presenting focused fullscreen window before drawing a single pixel.
2. **The Decision & Resolution:** Adopt Option (b) from the INC-7 audit: a grant made before the first map is accepted and recorded in `self.fullscreen`, but does **not** take over the output (`take_output(id)`) until the element maps with its first non-zero buffer commit.
3. **Ghost Newcomer Reaping:** If an unmapped newcomer requests fullscreen and is destroyed or unmapped before drawing, `drop_fullscreen(id)` reaps the waiting grant without the active presenter ever flickering.
4. **Waiting-grant ordering (as implemented):** grants are kept in the order they were made. At its map, an element's grant ends the older grants on its output (the presented one and older waiting ones); a newer waiting grant survives an older one's map and replaces it at its own map. A focused, mapped element's grant ends every older grant on the output, waiting ones included. A replaced element gets a configure with its saved size if it has one (a never-mapped waiting grant has none and gets a plain configure).
5. **Status:** UNRATIFIED. Sealed INV-PRES-1 still says the presentation starts at the grant decision; amendment layer C12.F6 is prepared for owner approval. Ported to `integration/inc-7-exit-2026-10-14` as `b1aa758`.

## Unverified Hypotheses
- None; the behavior is formally verified in unit tests and integration tests.

## Performance & Correctness Implications
- Prevents desktop blackouts and session disruption caused by slow-starting, crashing, or misbehaving fullscreen applications.

## Limitations & Counterexamples
- An unmapped newcomer is told it is fullscreen in its initial configure, but cannot present until it commits a buffer.

## Reproduction Instructions
Execute the pre-map fullscreen newcomer adversarial tests:
```bash
cargo test -p infynitae-backend-headless --test t1_inc7_adversarial a_pre_map_grant_takes_the_output_only_when_its_window_maps
cargo test -p infynitae-backend-headless --test t1_inc7_adversarial a_pre_map_grant_that_never_draws_ends_nothing
```

## Links & Citations
- Source Commit: [`07acfeb`](file:///home/vysaely/infynitae-native) (*"fix(inc-7): a grant made before the first map takes the output when the element maps (A-3)"*)
- Related Architecture: [KNOW-ARCH-001: Camera-Independent Fullscreen Presentation Projection](../architecture/KNOW-ARCH-001-camera-projection-independence.md)
- Related Pitfall: [KNOW-PITFALL-001: Physical Canvas Fullscreen Relocation](../pitfalls/KNOW-PITFALL-001-physical-canvas-fullscreen-relocation.md)
