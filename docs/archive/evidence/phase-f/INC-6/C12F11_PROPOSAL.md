# C12.F11 — F-SCALE-2: the device bound counts a size Core configures — PROPOSAL

Status: **PROPOSAL, NOT ADOPTED** (2026-10-15; text corrected to the candidate's code 2026-10-16, see the end). Evidence, not authority; no living document is
changed by it. Owner direction of 2026-10-15, item 2: "Device-bound scale decisions must account
for pending compositor-selected configure dimensions when those dimensions determine the next
client buffer." Implemented only on the rehearsal branch
`rehearsal/inc-6-scale-amendments-2026-10-15` (never merge before adoption).

## The contradiction

F-SCALE-2 (living, canonical; amended by C12.E1R and C12.F2) defines
`H_dev(E) = ⌊120 · D_max / s_max(E)⌋ / 120` with "`s_max(E)` = largest logical side over all
surfaces of E's tree". The direction needs `s_max` to include a size that is not yet in any
surface of the tree: the size of a configure Core has sent. Implementing it without this layer
would change a canonical formula silently.

## The defect it closes (OB-13 on the INC-6 × INC-7 integration)

A protocol-fullscreen grant (C12.F6, INV-PRES-1) asks the client for the output's logical size
(C5.2). The hint in force was bounded by the *windowed* tree. Measured (T0 probe, independent
review confirmed): W seen by a scale-3 output, `D_max` 8192, granted fullscreen on a 3840-px-wide
scale-1 output: configure 3840×2160 with integer hint 3, an obedient client's first buffer is
11,520 px wide, over the limit; P7 lowers the hint only after that buffer is committed. OB-13's
criterion "no buffer exceeds the limit Core was given" fails for that frame.

## Proposed canonical text

F-SCALE-2, the sentence defining `s_max(E)` becomes:

> `s_max(E)` = the largest logical side over all surfaces of E's tree **and over every size
> Core has asked E for while that request stands: the size of the protocol-fullscreen configure
> while E is granted (C5.2: the output's logical size), and the saved size offered in the
> configure that ended E's fullscreen (M9), until E commits a tree of another size, is unmapped
> or is destroyed** (0 iff none has a non-zero extent, e.g. every initial commit without a
> grant).

Everything else of F-SCALE-2, INV-SCALE-2 and P7 is unchanged. M15's row "first buffer attached
(`s_max` becomes > 0)" gains "or a grant's configure size".

## What it does not change

The ladder, `H_cfg`, P7's trigger and effect, the integer emission rule, SC-E, the epochs. A
client-chosen size (a windowed resize) still enters only with the buffer that has it.

**Correction of 2026-10-16 (text brought to the code of `4c81a1c`, no behaviour change):** this
section said "The saved size of an unset is the client's own earlier size and is not counted (the
fullscreen tree, larger, still bounds the hint until the client redraws)". That holds only when
the fullscreen tree is the larger. It is not when the window was larger than the output: a
3000 px window presented on a 1920 px output reaches hint 2, and the unset then asks it for
3000 px at hint 2 (6000 px, over `D_max` 4096). Since `4c81a1c` the offered size is counted, as
the canonical text above now says, until E redraws at another size (then its own tree bounds
it), is unmapped (C12.F10 drops the offer with the rest of the fullscreen state) or destroyed. A
re-grant before the redraw saves the size still on offer, not the fullscreen geometry. Tests
(T0, `infynitae-core`): `the_size_offered_at_fullscreen_exit_fits_the_device_limit`,
`a_spent_restore_offer_no_longer_bounds_the_hint`, `an_unmap_spends_the_restore_offer`,
`a_re_grant_before_the_redraw_offers_the_windowed_size_again`, each with a seeded control.

## Interaction

With owner direction item 3 (hint before configure, implemented on the resolution branch) the
lowered hint reaches the client before the fullscreen configure, so its first fullscreen buffer
fits. Without item 3 the order would still leave one frame at the old hint.
