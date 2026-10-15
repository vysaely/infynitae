# C12.F12 — F-SCALE-3: the initial hint of a pre-map fullscreen grant — PROPOSAL

Status: **PROPOSAL, NOT ADOPTED** (2026-10-15). Evidence, not authority; no living document is
changed by it. Owner direction of 2026-10-15, item 4: "When a pre-map fullscreen grant already
identifies an authoritative output, derive the initial scale hint from that output rather than an
unrelated canvas position." Implemented only on the rehearsal branch
`rehearsal/inc-6-scale-amendments-c12f11-f12` (never merge before adoption).

## The contradiction

F-SCALE-3 and INV-SCALE-5 (living, canonical): "**Initial hint:** `h := t(ρ_pred)` with ρ_pred =
F-SCALE-1 over the presentations whose view intersects the placed window-geometry rect (or the
anchor point when its size is 0×0); if none, `t(max connected output scale)`." A grant decided in
the initial configure (C12.F6) starts its presentation only at the element's first map
(INV-PRES-1), so at the initial commit that presentation does not exist and the formula cannot
count it. Counting it changes a canonical formula.

## Observed effect today

The initial hint follows the canvas placement; at the first map the presentation starts, the
subject is evaluated (fullscreen enter) and gets a second hint for the presenting output (T0
`a_pre_map_grant_takes_the_presenting_outputs_scale_when_it_maps`). A client renders its first
buffer at a scale the next event replaces.

## Proposed canonical text

F-SCALE-3 and INV-SCALE-5, "Initial hint", add:

> If the initial configure grants protocol fullscreen on output O (C12.F6), ρ_pred is F-SCALE-1
> over the presentation that grant starts at the first map (`scale_O`, zoom 1) and the camera
> presentations of the other outputs whose view intersects the placed rect (or anchor point):
> exactly the presentations that observe the element at its first map.

## What it does not change

The prediction of every element without a pre-map grant; M15's ordering (hint before the initial
configure); the evaluation at the first map (which then finds the hint already right and emits
nothing). With C12.F11, the device bound of that initial hint also counts the configured size.
