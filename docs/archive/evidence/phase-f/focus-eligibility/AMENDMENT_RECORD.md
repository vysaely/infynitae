# Focus eligibility and retention under a presentation (registry layer C12.F7) — record

Status: **adopted 2026-10-14 by owner approval; governance branch `governance/inc-7-layers-c12f5-f8`
from the integration candidate `93c2605`; not merged, not pushed.** Evidence and provenance; the
authority is FOUNDATION_ARCHITECTURE §C12.F7. Adopting this layer does not ratify INC-7.

## What is adopted

INV-INPUT-8 and STATE_MACHINES M11 (two rows reworded, one guard, one row added). Keyboard focus is
never moved to an element hidden by a presentation. A focused element that becomes hidden keeps the
focus. A fullscreen request or grant gives no focus.

"Hidden by a presentation" is the implemented predicate (`Core::hidden_by_presentation`): an output
presents another element, the camera bound to that output would show this element, and no output
observes it. An element that no camera would show anyway is not hidden by a presentation and stays
eligible, as without one.

## Approved requirements and where the wording carries them

| Owner requirement (2026-10-14) | Wording |
|---|---|
| forbid new keyboard focus moves to windows hidden by fullscreen | INV-INPUT-8 "never to an element hidden by a presentation … such an action is declined"; M11 guard and "declined; no effect" row |
| preserve existing focus when visibility changes, even if the focused window becomes hidden | INV-INPUT-8 "keeps it when a presentation or an output change hides it"; M11 "unchanged (a focused element that becomes hidden keeps the focus)" |
| a fullscreen request or grant alone never steals focus | INV-INPUT-8 "requesting or obtaining fullscreen gives a client no focus"; M11 "fullscreen request or grant → unchanged" |
| preserve existing authorized focus transitions | the explicit actions and target loss are unchanged for eligible elements; without a presentation every mapped element is eligible |
| supersede the conflicting M11 transitions | C12.F7.1 |

The M11 target-loss row says "successor, chosen among the elements not hidden by a presentation"
(the approved INV-INPUT-8 wording), not the supplementary draft's "the most recently focused
element …". That draft restated the successor policy itself, which this layer does not change.

## Implementation and tests (on `93c2605`; no production code changes in this layer)

`0f45cfd` (A-8; on this line the observed test is `!self.membership.observers(element).is_empty()`
instead of INC-6's `observing_outputs`, the same predicate) and `2aa8d65` (no per-element check
without a presentation). `set_focus` and `grant_initial_focus` return no effect for a hidden target;
FocusNext and `target_lost` use `focusable_elements()`; `toplevel_request` only reads the focus target and never changes it.

| Property | Test |
|---|---|
| set_focus, initial grant, FocusNext and successor skip or decline hidden elements | T0 `focus_does_not_move_to_an_element_a_presentation_hides` |
| on the wire, with real clients' surfaces | T1 `focus_does_not_move_to_a_window_the_presentation_hides` |
| the presented window's own menu chain keeps working | T1 `a_menu_chain_of_a_fullscreen_window` |
| an unfocused client's request is declined and changes nothing | T1 `an_unfocused_client_is_declined_and_nothing_changes` |
| retained focus on a window hidden by a pre-map takeover or an output removal | exit-review probes R1, R2 (`~/Infynitae-INC7-EXIT-REVIEW-2026-10-14/evidence`) |
| seeded controls of `0f45cfd` (4) | `tools/verify-test-efficacy` (231/231 on this branch) |

## Residual (owner policy 2026-10-14)

A focused element hidden by a presentation keeps the focus and receives keyboard input while hidden;
the visible presenter is not activated. Recorded as INC-7 residual R-01; a "move focus when a
presentation starts" policy may be decided with the shell (INC-11). On a single output nothing moves
focus away from a presented window except leaving fullscreen (R-13).
