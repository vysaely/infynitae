# Default placement at the first windowed presentation (registry layer C12.F5) — record

Status: **adopted 2026-10-14 by owner approval; governance branch `governance/inc-7-layers-c12f5-f8`
from the integration candidate `93c2605`; not merged, not pushed.** Evidence and provenance; the
authority is FOUNDATION_ARCHITECTURE §C12.F5. Adopting this layer does not ratify INC-7.

## What is adopted

INV-STAGE-1 clause (a), F-PLACE-1, STATE_MACHINES M1 (two rows) and PROTOTYPE_DECISION_LEDGER
PL-21: the default placement is resolved once, at the element's first windowed presentation. That is
its first map, unless the element is in protocol fullscreen then. In that case it is the first commit
that applies an acknowledged configure without the fullscreen state and is made while the element is
not in protocol fullscreen. Protocol fullscreen never writes an anchor.

## Approved requirements and where the wording carries them

| Owner requirement (2026-10-14) | Wording |
|---|---|
| supersede PL-21's first-map statement | PL-21 "at its first windowed presentation (INV-STAGE-1)"; C12.F5.1 row 4 |
| the qualifying first windowed presentation after acknowledged fullscreen exit | INV-STAGE-1 "the first commit that applies an acknowledged configure without the fullscreen state" |
| the element is out of fullscreen at that commit | INV-STAGE-1 "and is made while the element is not in protocol fullscreen" |

## Implementation and tests (on `93c2605`; no production code changes in this layer)

`334b9b3` (P9b). `Core::fullscreen_ended` marks an element that was never placed as pending its
windowed placement. The commit path places it (`place_by_default`, through `stage::place`) when the
commit carries the client's acknowledged windowed state and the element is not in protocol
fullscreen. `Stage::default_placed` makes the placement happen once.

| Property | Test |
|---|---|
| no placement before the exit is acknowledged; placement at the first windowed commit after it | T1 `placement_after_fullscreen_waits_for_the_acknowledgement` |
| a client that keeps the fullscreen size as its windowed size is placed at its first windowed commit | T0 `a_window_keeping_the_fullscreen_size_is_placed_at_its_first_windowed_commit` |
| a re-grant before the windowed commit postpones placement again (out of fullscreen at the commit) | exit-review probe R5 (`~/Infynitae-INC7-EXIT-REVIEW-2026-10-14/evidence`) |
| seeded controls of `334b9b3` | `tools/verify-test-efficacy` (231/231 on this branch) |

## Not changed

The F-PLACE-1 formula itself, the placement output, the provisional anchor at initial commit,
INV-STAGE-5, INV-CAMERA-2/-4, and an element first mapped windowed, which is still placed at its
first map.
