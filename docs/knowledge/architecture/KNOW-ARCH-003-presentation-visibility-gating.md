# KNOW-ARCH-003: Presentation Visibility Gating for Keyboard Focus

| Field | Value |
| :--- | :--- |
| **id** | KNOW-ARCH-003 |
| **title** | Presentation Visibility Gating for Keyboard Focus |
| **subsystem** | Core/Focus |
| **source_rev** | 0fe8f06 / 489180e |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | All Phase F / INC-7+ branches |
| **affected_branches** | experimental/inc7-exit-readiness-2026-10-14 |
| **related_invariants** | INV-INPUT-8, INV-PRES-1 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | Maintain benchmark ratchet in core_bench |

---

## Verified Facts
1. In traditional compositors (including DriftWM [`src/state/keyboard_focus.rs:106`](file:///home/vysaely/.gemini/antigravity/scratch/driftwm/src/state/keyboard_focus.rs#L106)), programmatic focus changes allow keyboard focus to be assigned to occluded windows hidden behind an active fullscreen presentation.
2. Infynitae introduced `hidden_by_presentation(&self, element: ElementId) -> bool`: an element is considered hidden if an output presents another element, the output camera would show the element, and no other active output observes it.
3. In commit `489180e`, `hidden_by_presentation` was optimized with `!self.presenting()` early-exit check, returning in $\mathcal{O}(1)$ when no fullscreen presentation is active.

## Unverified Hypotheses
- Hypothesized that multi-monitor environments where an occluded window spans into a second active output should remain focusable; verified by test `focus_does_not_move_to_an_element_a_presentation_hides`.

## Performance & Correctness Implications
- Eliminates the security risk of keystroke hijacking by invisible windows behind fullscreen applications.
- Zero runtime overhead in steady-state desktop operations without active presentations.

## Limitations & Counterexamples
- An element visible on a secondary output remains eligible for keyboard focus even if occluded on the primary presentation output.

## Reproduction Instructions
Execute the focus presentation test suite:
```bash
cargo test -p infynitae-core --lib focus_does_not_move_to_an_element_a_presentation_hides
```

## Links & Citations
- Source Commits: [`0fe8f06`](file:///home/vysaely/infynitae-native), [`489180e`](file:///home/vysaely/infynitae-native)
- Related Decision: [KNOW-DEC-002: Fullscreen Focus Eligibility](../decisions/KNOW-DEC-002-a8-fullscreen-focus-eligibility.md)
