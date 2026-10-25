# Popup stacking follows xdg-shell (registry layer C12.F8) — record

Status: **adopted 2026-10-14 by owner approval; governance branch `governance/inc-7-layers-c12f5-f8`
from the integration candidate `93c2605`; not merged, not pushed.** Evidence and provenance; the
authority is FOUNDATION_ARCHITECTURE §C12.F8. Adopting this layer does not ratify INC-7 and does not
close OB-1.

## What is adopted

INV-RENDER-2 (the layer's invariant row). Through the layer's table it also supersedes C3.3's
introductory sentence, C3.3 item 1, Obligation X-15 and the historical OB-1 row of the CP3A-4
registry. The living OB-1 item (FOUNDATION_TEST_PLAN) and the W-SM-20 record (FOUNDATION_BLOCKERS)
are edited in place. A toplevel's `xdg_popup`s are stacked newest first, as xdg-shell requires.
Within each popup's own surface tree, and for the root tree, Smithay's order is kept.

## Approved requirements and where the wording carries them

| Owner requirement (2026-10-14) | Wording |
|---|---|
| supersede INV-RENDER-2's popup ordering | INV-RENDER-2 (C12.F8) |
| require newest-first `xdg_popup` stacking | INV-RENDER-2 "the toplevel's `xdg_popup`s newest first (xdg-shell; W-SM-20)"; X-15 (amended) |
| supersede the historical X-15, C3.3 and OB-1 text through the layer mechanism | C12.F8.1 (historical text not edited); living OB-1 item edited in place |

The C3.3, X-15 and OB-1 replacement texts are those of the approval package, verbatim. The living
OB-1 item adds the pointer "(order criterion amended by FOUNDATION_ARCHITECTURE §C12.F8)".

## Implementation and tests (on `93c2605`; no production code changes in this layer)

`23860b3` (P10 #3): the adapter's `new_popup` numbers each `xdg_popup` at creation
(`set_popup_created`); the snapshot sorts a toplevel's popups by that number, newest first, with a
stable sort (a popup not created through `new_popup` keeps Smithay's order). A child is always newer
than its parent, so it stays in front of it. `d350ed2` re-anchors one seeded control on the new loop.

| Property | Test |
|---|---|
| overlapping siblings and a child created after its parent's newer sibling: every pixel and the hit-test follow the protocol | T1 `a_newer_popup_is_stacked_above_every_older_one` |
| three-level chain, sibling, subsurface, scale 1 and 1.25: order equals xdg-shell's; Smithay's order differs by exactly the newest popup (so an upstream change shows) | T1 `ob1_nested_popup_chain_matches_what_is_presented` |
| seeded controls (2 + 1 re-anchored) | `tools/verify-test-efficacy` (231/231 on this branch) |

## Not changed

Element order (reverse Stage z-order), the root tree after the popups, the downward traversal, the
popup offset formula, hit-testing through the presented order (OB-26). Core is unchanged. The source
comment in `crates/infynitae-wayland/src/snapshot.rs` still says "W-SM-20, proposed; X-15 amendment
pending"; it is left as is because this layer changes no code.
