# A pre-map grant takes the output when its element maps (registry layer C12.F6) — record

Status: **adopted 2026-10-14 by owner approval; governance branch `governance/inc-7-layers-c12f5-f8`
from the integration candidate `93c2605`; not merged, not pushed.** Evidence and provenance; the
authority is FOUNDATION_ARCHITECTURE §C12.F6. Adopting this layer does not ratify INC-7.

## What is adopted

INV-PRES-1, third paragraph, and STATE_MACHINES M9 (two rows reworded, two rows added, two rows
qualified with "if available"). A fullscreen grant takes its output when its element is mapped.
Until then the presented element keeps the output and is told nothing. Grants are ordered by the
time they were made. The configure that ends a grant offers the saved size, if one was saved.

## Approved requirements and where the wording carries them

| Owner requirement (2026-10-14) | Wording |
|---|---|
| deferred presentation takeover until mapping | INV-PRES-1 "The presentation starts when the granted element is mapped"; M9 rows 1 and 2 |
| preserve the current presenter until the replacement actually takes over | INV-PRES-1 "until then the presented element keeps the output and is told nothing"; M9 row `Fullscreen{O} by W` "a newer grant still waiting leaves W unchanged" |
| waiting-grant ordering exactly as implemented | INV-PRES-1 "Grants are ordered by the time they were made … never by the map of an older one"; M9 row 2 "grants on O older than W's end" |
| optional saved-size semantics | INV-PRES-1 "if available"; M9 rows `unset_fullscreen / unmap`, `Fullscreen{O} by W`, `O disconnected` |
| supersede the conflicting M9 transitions | C12.F6.1 |

## Wording-level reconciliations of the supplementary text (2026-10-14)

- M9 "W destroyed before its map" (the supplementary text said "destroyed or unmapped"): an element
  waiting for its first buffer cannot be unmapped (M1). The approved INV-PRES-1 sentence is kept
  verbatim; its "or unmapped" part has no reachable case today.
- M9 `Fullscreen{O} by W`: the clause "an older waiting grant never replaces W" is omitted. A waiting
  grant older than W's is ended when W takes the output, so the case cannot arise.
- "if available" on the unset, unmap and output-removal rows carries the approved optional saved
  size. A size is saved at a grant from the element's windowed size; a grant made before the first
  map has none, and the configure then carries no size (`restore_effect`).
- "presentation ends, if it had started": unset of a waiting grant ends no presentation.

## Implementation and tests (on `93c2605`; no production code changes in this layer)

`b1aa758` (A-3). `store_fullscreen` pushes the grant in order and calls `take_output` only for a
mapped element; the commit that maps a waiting element calls `take_output`; `take_output` ends the
grants on the element's output that are older than its own (presented or waiting), each through
`fullscreen_ended`; `drop_fullscreen` forgets a destroyed element's grant without effects.

| Property | Test |
|---|---|
| a pre-map grant leaves the presenter untouched until the element maps, then takes the output | T0 `a_pre_map_grant_takes_the_output_when_its_element_maps`; T1 `a_pre_map_grant_takes_the_output_only_when_its_window_maps` |
| a pre-map grant whose client never draws ends nothing | T1 `a_pre_map_grant_that_never_draws_ends_nothing` |
| a newer grant replaces the older one; focus loss does not | T0 `a_newer_grant_replaces_the_older_one_and_focus_loss_does_not` |
| no saved size → configure without a size | exit-review probe R6 (`~/Infynitae-INC7-EXIT-REVIEW-2026-10-14/evidence`) |
| seeded controls of `b1aa758` (3) | `tools/verify-test-efficacy` (231/231 on this branch) |

## Not changed

The first two paragraphs of INV-PRES-1, the output choice, F-PRESENT-1, INV-OUTPUT-1.
