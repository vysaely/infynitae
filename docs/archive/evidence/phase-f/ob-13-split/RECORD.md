# OB-13 split — record

Status: **adopted 2026-10-14 by owner decision; local main, not pushed.** Neither obligation is
closed by the split, and the INC-6 exit is not thereby authorized.

## The original, and why it could not be closed as one

"Runtime `D_max` query and enforcement. Test/decision: query from the renderer at startup;
assert h ≤ H_max for large windows." Pass criterion: no buffer exceeds the device limit.
Destination T1; blocks the INC-6 exit. INC-13 cited it as "OB-13 (real `D_max`)".

One identifier carried a headless requirement (INC-6) and a hardware requirement (INC-13).
Closing it at the INC-6 exit would have discharged the hardware part without hardware evidence.

## The two obligations

| | OB-13 (INC-6) | OB-52 (INC-13) |
|---|---|---|
| Requirement | Core enforces the capability it is given. The backend obtains it from its renderer through the renderer capability interface. A headless fixture may report a documented deterministic limit. No unexplained constant goes directly into Core | the limit is queried from the real renderer and device, at startup and after renderer recreation, propagated through the same interface, and enforcement is shown on a real GPU |
| Destination | T1 | T2 |
| Closable by | a T1 production test | a nested run on a real GPU; no fixture |
| Blocks | INC-6 exit | INC-13 exit |

## Completeness check: nothing of the original is dropped

| Clause of the original | Where it is now |
|---|---|
| "enforcement": h ≤ H_max for large windows | OB-13 |
| "no buffer exceeds the device limit" | OB-13 against the reported limit; OB-52 against the real one |
| "query from the renderer at startup" | OB-13: through the renderer interface, a deterministic fixture allowed; OB-52: the real renderer |
| E1R additions (zero-extent domain, integer hint) | OB-13 |
| re-query on renderer recreation (INV-SCALE-2) | OB-52 |

## Does it weaken an exit requirement?

No. INC-6's objective already reads "`D_max` queried from the (Dummy) renderer": the INC-6 exit
still requires the query, now stated as going through the renderer interface. It becomes
stricter in one respect: a constant passed straight into Core, which is what both run loops did,
no longer satisfies it. INC-13's exit still requires the real limit, under its own identifier,
and the docs checker now refuses to see OB-52 closed (destination T2 is a deferred destination).

## Implementation

The capability interface and the headless fixture are INC-6 work, on the INC-6 experimental
line. Nothing on main implements them, because the scale policy is not on main.
