# KNOW-DEC-003: Default Placement After Fullscreen Acknowledgement (Finding P9b)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-DEC-003 |
| **title** | Default Placement After Fullscreen Acknowledgement (Finding P9b) |
| **subsystem** | Core/Placement |
| **source_rev** | df72202 |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | Toplevel lifecycle and default placement |
| **affected_branches** | experimental/inc7-exit-readiness-2026-10-14 |
| **related_invariants** | F-PLACE-1, INV-STAGE-1, C12.F1 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | Ratify registry layer C12.F5 in governance |

---

## Verified Facts
1. **The Flaw of Old Rule P9:** Rule P9 triggered default placement on the first commit whose size differed from the fullscreen size. Consequently, a client that acknowledged the fullscreen exit but chose to maintain its windowed dimensions at the full display size was never placed, and stale frames already drawn at a windowed size triggered placement prematurely.
2. **The P9b Decision:** Default placement after fullscreen exit must wait for the client's explicit **acknowledgement** of the non-fullscreen state:
   - Placement runs at the first commit after the client acknowledged a configure without the fullscreen state, regardless of its buffer size.
   - Core tracks this via `toplevel_committed_acked(id, tree, acked_fullscreen)`.
   - The adapter extracts this from Smithay's `ToplevelSurface::current_state()` in the post-commit hook before invoking `CompositorHandler::commit`.

## Unverified Hypotheses
- None; verified via adversarial multi-frame tests with stale in-flight buffers.

## Performance & Correctness Implications
- Guarantees deterministic, idempotent window placement even when clients do not resize buffers upon fullscreen exit.

## Limitations & Counterexamples
- An element with an already resolved or existing anchor does not trigger placement; placement is one-time per element lifecycle.

## Reproduction Instructions
Execute the P9b placement acknowledgement tests:
```bash
cargo test -p infynitae-backend-headless --test t1_inc7_adversarial placement_after_fullscreen_waits_for_the_acknowledgement
```

## Links & Citations
- Source Commit: [`df72202`](file:///home/vysaely/infynitae-native) (*"feat(inc-7): default placement after fullscreen at the first commit after the exit is acknowledged (P9b)"*)
- Related Decision: [KNOW-DEC-001: Pre-Map Fullscreen Ownership](KNOW-DEC-001-a3-premap-fullscreen-ownership.md)
