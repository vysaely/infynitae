# OB-34 (cross-output drag, F-DRAG-1) — owner decision record (2026-10-22)

**Question asked** (2026-10-22, after the owner's first console run asked for Super+left-drag move
and Super+right-drag resize "like Hyprland"): when a window is dragged from one monitor to another,
each with its own camera, what should it do?

- **(a) Stays under the cursor.** The window keeps the point where it was grabbed under the mouse,
  on whichever monitor the mouse is. On the Canvas it jumps to the part the new monitor shows. This
  is F-DRAG-1's own candidate: `anchor = R3(q_now − grab_offset)`.
- **(b) Moves by the mouse distance.** The anchor moves by the Canvas deltas summed since the drag
  start. Across monitors whose cameras differ, the window can end up away from the cursor.

**Owner's answer, verbatim:** "if stays under the cursor basically mean it can just teleport from
one monitor 1 viewport to the another viewport 2 then yes" — and that is what (a) means.
**Decided: (a).**

## Consequence for F-DRAG-1 (not edited in place)

- **Change:** the open part of F-DRAG-1 is decided as
  `anchor = R3(q_now − grab_offset)`, where:
  - `q_now` is the pointer's Canvas position (R1, through the camera of the output it is on);
  - `grab_offset = q_press − anchor₀`, kept for the whole drag.
- **Within one output at a fixed camera** this equals the current definition
  `R3(anchor₀ + Q16(Σ Canvas deltas))`.
- **Status:** the formula text belongs to change control; a layer recording it is prepared with
  INC-14 and is **not adopted**.
- **OB-34 status:** OPEN until change control records the decision ("may be closed by:
  architecture change control only"). G-OB34 is met in substance by this decision; adopting it,
  and closing OB-34, are owner acts.

## What the decision unblocks

INC-14 (interactive move/resize) may start: entry conditions G-OB34 (this record), INC-5 and INC-6
exits (both ratified).

Work begun before this decision: the first move/resize code on
`feature/tty-backend-2026-10-22` was written before G-OB34 was checked. It was held, not merged,
and is reworked to this decision.
