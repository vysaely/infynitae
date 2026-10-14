# KNOW-DEC-002: Keyboard Focus and Windows Hidden by a Fullscreen Presentation (Finding A-8)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-DEC-002 |
| **title** | Keyboard Focus and Windows Hidden by a Fullscreen Presentation (Finding A-8) |
| **subsystem** | Core/Focus |
| **source_rev** | 0fe8f06 / 489180e (source line); 0f45cfd / 2aa8d65 on main; layer C12.F7 37bed72; ratified 39ad614 |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | UNRATIFIED. Implemented on experimental/inc7-exit-readiness-2026-10-14 and integration/inc-7-exit-2026-10-14; sealed INV-INPUT-8 still lacks it (amendment layer C12.F7 prepared, not adopted) |
| **affected_branches** | main (f323c2d); historical: experimental/inc7-exit-readiness-2026-10-14, integration/inc-7-exit-2026-10-14 |
| **related_invariants** | INV-INPUT-8, INV-PRES-1 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | Owner decision on C12.F7 (owner-selected policy of 2026-10-14: reject transfers, keep retention, document it); revisit clearing focus on presentation start at INC-11 |

---

## Verified Facts
1. **Definition.** An element is *hidden by a presentation* when an output presents another element, the camera bound to that output would show this element (F-MEMBER-1 under that camera's view), and no output observes it. Implemented as `Core::hidden_by_presentation`.
2. **Transfers are refused.** `set_focus` and `grant_initial_focus` decline a hidden target (no change, no effect); FocusNext and the successor after a target loss choose among `focusable_elements()`. Click-to-focus and pointer focus cannot reach a hidden element anyway: hit-testing goes through the presentation's view.
3. **Retention is kept (owner policy, 2026-10-14).** A focused element that *becomes* hidden keeps the focus and keeps receiving keyboard input: (R1) a window granted fullscreen before its first map maps and takes the output while W is focused — W is hidden, focused, receives the keys, and the visible presenter is not activated; (R2) a window focused because a second output showed it becomes hidden when that output is removed. This follows sealed INV-INPUT-8 "Camera, visibility … never change it".
4. **No focus from fullscreen.** Requesting or obtaining fullscreen gives a client no focus (moving focus to the presenter at takeover would let any client take keyboard focus by asking for fullscreen before its first map).
5. **Cost.** With no presentation, `presenting()` makes both functions return at once: unmap + remap at 3,000 windows 47 µs, as before A-8 (the unguarded first version cost 122 µs).
6. **Integration adaptation.** On the INC-7 integration line, where INC-6's `MembershipTable::observing_outputs` does not exist, the predicate uses `!observers(element).is_empty()` (same predicate, one small allocation per check while presenting).

7. **Reconciliation 2026-10-14 (main `f323c2d`):** C12.F7 (INV-INPUT-8: no focus move to an element hidden by a presentation; retained focus when hidden; no focus from fullscreen requests; M11) adopted (`37bed72`) and **ratified with INC-7** (`39ad614`). Retention is accepted residual **R-01** (owner policy) and single-output stickiness is **R-13**; both are tracked follow-up work, not resolved defects (CHECKPOINT `f323c2d`). The `observers()` allocation per hidden check is **R-15** (converges when INC-6 integrates).

## Unverified Hypotheses
- Whether users expect focus to follow the visible presenter after a pre-map takeover (R1) is a UX question, not measured.

## Performance & Correctness Implications
- Keys can no longer be *moved* to an invisible window by a binding, a shell request or a successor choice.
- Keys can still *continue* to an invisible window that was focused before it was hidden (R1, R2): text typed then — secrets included — reaches a window the user cannot see. One click on the visible presenter moves the focus.
- With one output, nothing moves the focus away from a presented window except leaving fullscreen; a window shown on another output stays focusable.

## Limitations & Counterexamples
- Retention (Fact 3) is a deliberate limitation, recorded as residual R-01 of the INC-7 exit preparation; clearing the focus when a presentation hides the focused window would need INV-INPUT-8's "visibility never changes it" amended.
- An element far from every camera is not "hidden by a presentation" and stays focusable, as without one.

## Reproduction Instructions
```bash
cargo test -p infynitae-core --lib focus_does_not_move_to_an_element_a_presentation_hides
cargo test -p infynitae-backend-headless --test t1_inc7_adversarial focus_does_not_move_to_a_window_the_presentation_hides
```
Retention probes R1–R3 (scratch, not in the repository): `~/Infynitae-INC7-EXIT-REVIEW-2026-10-14/evidence/review_probes.rs`, output `review_probes_output.txt`.

## Links & Citations
- Related Architecture: [KNOW-ARCH-003: Presentation Visibility Gating](../architecture/KNOW-ARCH-003-presentation-visibility-gating.md)
- Related Decision: [KNOW-DEC-001: Pre-Map Fullscreen Ownership](KNOW-DEC-001-a3-premap-fullscreen-ownership.md)
- Related Pitfall: [KNOW-PITFALL-002: Global Focus Pull Loops](../pitfalls/KNOW-PITFALL-002-global-focus-pull-loops.md)
- Integration provenance: [KNOW-DEC-005: INC-7 Integration Provenance](KNOW-DEC-005-inc7-integration-provenance.md)
