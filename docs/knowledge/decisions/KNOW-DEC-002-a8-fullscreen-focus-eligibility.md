# KNOW-DEC-002: Rejection of Keyboard Focus on Occluded Windows (Finding A-8)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-DEC-002 |
| **title** | Rejection of Keyboard Focus on Occluded Windows (Finding A-8) |
| **subsystem** | Core/Focus |
| **source_rev** | 0fe8f06 / 489180e |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | Focus policy under fullscreen presentation |
| **affected_branches** | experimental/inc7-exit-readiness-2026-10-14 |
| **related_invariants** | INV-INPUT-8, INV-PRES-1 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | Maintain test efficacy controls in verify-test-efficacy |

---

## Verified Facts
1. **The Policy Vulnerability (Finding A-8):** Under naive fullscreen focus handling, focus loss did not end fullscreen, and programmatic or binding navigation (`FocusNext`, Alt-Tab, mouse hover) could move keyboard focus to a window occluded behind an active fullscreen presentation.
2. **The Decision:** Keyboard focus must **never** move to an element hidden by an active fullscreen presentation unless that element is currently visible on another active display output.
3. **Implementation:** Added `Core::hidden_by_presentation(element)` and filtered all focus candidate lists (`focusable_elements()`). In commit `489180e`, added early-exit check `!self.presenting()` to ensure zero overhead in normal desktop sessions.

## Unverified Hypotheses
- None; the behavior is enforced with 4 seeded mutation controls in `tools/verify-test-efficacy`.

## Performance & Correctness Implications
- Prevents typing into invisible background windows, avoiding unintended data leaks or terminal command execution.
- Optimized in `489180e` so unmap/remap operations at 3,000 windows run in $47\,\mu\text{s}$ (down from $122\,\mu\text{s}$ during unoptimized checks).

## Limitations & Counterexamples
- An element occluded on Output A but rendered inside the visible viewport of Output B remains eligible for focus.

## Reproduction Instructions
Execute the focus presentation test suite:
```bash
cargo test -p infynitae-core --lib focus_does_not_move_to_an_element_a_presentation_hides
```

## Links & Citations
- Source Commits: [`0fe8f06`](file:///home/vysaely/infynitae-native), [`489180e`](file:///home/vysaely/infynitae-native)
- Related Architecture: [KNOW-ARCH-003: Presentation Visibility Gating](../architecture/KNOW-ARCH-003-presentation-visibility-gating.md)
- Related Pitfall: [KNOW-PITFALL-002: Global Focus Pull Loops](../pitfalls/KNOW-PITFALL-002-global-focus-pull-loops.md)
