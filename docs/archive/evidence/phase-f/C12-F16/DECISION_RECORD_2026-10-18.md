# C12.F16 — owner decision record (2026-10-18)

Status: **owner decision RECORDED; layer C12.F16 ADOPTED on local `main` (base `da538d6`).
Governance and documentation only. Not pushed. OB-24 stays OPEN. INC-10 is NOT started.**

## 1. Authority

Owner brief "Ratify Bounded Fit/Restore History" (2026-10-18), deciding the depth bound that
C12.F15.9 left open:
- at most **256 successful fit operations per output** in the fit/restore history, a bounded stack
  of exact prior camera values;
- each successful fit records the exact camera value immediately preceding it; restore pops and
  applies the newest retained prior value; repeated restores unwind retained fits in reverse
  chronological order;
- a restore with an empty history is refused without mutating the camera or the history;
- when a successful fit would exceed capacity, the oldest entry is evicted before the new prior
  value is retained — after more than 256 successful fits without enough restores the oldest
  history is unavailable, and no sequence of restores recovers it;
- history for one output is never applied to another output; storage stays bounded.

The brief authorizes the governance layer and its record commit only — not INC-10, not production
code, not a push.

## 2. Verification before writing (local `main` = `da538d6`, clean; `origin/main` = `23752f1`)

| Item | Verified against | Result |
|---|---|---|
| Output identity | `infynitae-core/src/output.rs:122-131` | `OutputIds::next` increments a `u64`; an `OutputId` is never reused |
| Camera state | `infynitae-core/src/navigation.rs:13-20`, `geom.rs:40-43` | value = `center: CanvasPos` (two `i64`, Q16) and `zoom: f64`; `bound: Option<OutputId>`; cameras survive outputs (INV-CAMERA-3) |
| Lifecycle authority | `infynitae-core/src/entry.rs:428-445, 471-475, 516-530` | `output_connected` allocates a new id and mints a proof for `bind_on_connect`; `output_disconnected` mints one for `unbind_on_disconnect`; `output_reconfigured` mints none |
| Re-binding | `navigation.rs:111-151` | a reconnect with the same `OutputKey` re-binds the lowest unbound remembered camera under the **new** `OutputId` |
| Fit/restore code | `git grep FitViewport ViewportRestore` | none: INC-10 not started; no production history exists |

Consequence: a history kept per camera would carry a previous connection's entries into a new
`OutputId`. The safe rule, needing no new identity mechanism, is to empty the history in
`unbind_on_disconnect` under the unbinding's existing proof (C12.F16.3 items 5–6, C12.F16.4).
Reconfiguration mints no proof and keeps the `OutputId`, so it keeps the history; a recorded value is
Canvas-space and valid for any descriptor.

## 3. Conflicts checked

None found. INV-CAMERA-1 (history writes inside `navigation`, each under its action's proof; the
lifecycle entries still never change a camera's value); INV-CAMERA-3 and INV-OUTPUT-1 (value kept on
unbinding — the history is not the value); INV-OUTPUT-2 (reconfiguration mints nothing and changes
no history); INV-CAMERA-5 (restore still uses a value recorded at fit time); F-FIT-1 (guard, zoom,
clamp, centre unchanged); INV-ALLOC-1 (fixed storage; fit and restore are not among its hot paths).

## 4. Recorded as

FOUNDATION_ARCHITECTURE §C12.F16 (INV-CAMERA-5 row; F-FIT-1 record; the contract, lifecycle,
compatibility, the required tests and seeded controls); living F-FIT-1; FOUNDATION_INVARIANTS
INV-CAMERA-5; OBJ-30; M5 and M8; ADR-04; FOUNDATION_IMPLEMENTATION_PLAN INC-10 (entry condition,
exit item 11, negative controls); CHECKPOINT; `tools/check-foundation-docs` (`F_LAYERS`, fixture 46).
The tests are specified, not written: they must exercise the production history, which INC-10 adds.
