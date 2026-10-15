# C12.F11 — F-SCALE-2: the device bound counts a size Core configures — PROPOSAL

Status: **PROPOSAL, NOT ADOPTED** (2026-10-15). Evidence, not authority; no living document is
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

> `s_max(E)` = the largest logical side over all surfaces of E's tree **and over the size of the
> protocol-fullscreen configure Core has sent E while E is granted (C5.2: the output's logical
> size)** (0 iff neither has a non-zero extent, e.g. every initial commit without a grant).

Everything else of F-SCALE-2, INV-SCALE-2 and P7 is unchanged. M15's row "first buffer attached
(`s_max` becomes > 0)" gains "or a grant's configure size".

## What it does not change

The ladder, `H_cfg`, P7's trigger and effect, the integer emission rule, SC-E, the epochs. A
client-chosen size (a windowed resize) still enters only with the buffer that has it. The saved
size of an unset is the client's own earlier size and is not counted (the fullscreen tree, larger,
still bounds the hint until the client redraws).

## Interaction

With owner direction item 3 (hint before configure, implemented on the resolution branch) the
lowered hint reaches the client before the fullscreen configure, so its first fullscreen buffer
fits. Without item 3 the order would still leave one frame at the old hint.
