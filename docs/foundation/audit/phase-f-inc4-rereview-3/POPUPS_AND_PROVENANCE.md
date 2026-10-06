# Fourth Review — Popups, Pan Grab, Reconnect, Provenance

## popup_destroyed / element_committed (R2-01 → R3-B1)
`element_committed` = `window.on_commit()` + snapshot extraction from **committed** state (no pending state exposed) + dismissal of popups if the tree is unmapped + **`Core::toplevel_committed`** (the M1 lifecycle machine). Calling it on destruction therefore re-runs lifecycle transitions without a client commit:
- unmapped parent (U01c/C01c/D01): Unmapped → AwaitingFirstBuffer + configure on destroy; the client's later re-initial commit is answered with **no** configure — window cannot be re-shown (introduced by round 3; `6819c8a` answers 1).
- parent that never committed (U01b/C01b): configured on popup **commit** already — pre-existing INC-1 behaviour (O-1), not attributable to round 3.
- `else` branch commits every element when the root cannot be resolved (U01d did not reach it; unproven risk).
Pending-geometry, frame/damage and subsurface-sync effects: none observed (extraction reads committed state; frame production unchanged in U01e/U23).
Former-area clicks fixed (U01a, U01e).

## PanDismissedPopupGrab (R2-05) — RESOLVED
State machine: created at CameraPan acquisition when the popup pointer grab (install serial) is active (or any grab — unreachable alternatives during a valid acquisition); Focus::Keep; consumes nothing during the pan (Core never calls the adapter for pan motion); first post-pan pointer event (true-position motion, relative, button, axis, gesture begin) → `unset_grab(restore_focus=false)` then forwards that event; zero-output begin replaces it (no-op unset); frame passes through. Attacks (U05): destroy during pan, nested inner destroy, three-level destroy, focus change, Esc cancel — no client events during the pan, grab gone after the first motion, clicks delivered; U05b 30 cycles clean. Zero outputs + reconnect during the pan loses clicks identically without a popup (C05) — not INC-4.

## Three-level trees (U09)
Focus to another root, CameraPan, zero outputs + reconnect: no residual grabs, clicks delivered, 0 violations (install-serial keyed preemption; F11 detected). Client destroying P3, P2, P1: keyboard grab left → R3-A1.

## Reconnect (U10) — no resurrection
destroy→disconnect→reconnect, disconnect→destroy→reconnect, nested destroy while disconnected, root destroyed while disconnected, 5× repeated: no pointer grab resurrected; clicks delivered. Keyboard grab left after client-destroyed nested popups (R3-A1). F10 (liveness check removed) equivalent — `cleanup()` + `has_ended()` already prevent resurrection.

## Serial store and rule 1 (R2-07/R2-10)
Bounded at 64 (shipped test; F14 detected). Semantics (reviewer U12/U12b/U12c): newest entry after >64 events resolves (A → 360); an A serial still within the window after overflow resolves (360); evicted and missing serials resolve to the current pointer output (−200), never to the latest record (40). Shipped suite does not test these: F15 (evict newest) and F16 (unknown → latest record) **survive** (R3-B3). Rule-1 fallback = current pointer output, used *as* rule 1 ("seat's last input"); the round-3 record calls it "Rule 2", which canonically is the frame-clock output (R3-C1). Keyboard-caused provenance is not recorded (falls back the same way) — documented, not accidentally implemented elsewhere.

## Ordered preemption / W-SM-7/14/15
Order intact in source. N09f and N09e detected by default tests; **N09d (pointer-first, W-SM-15) survives** the default suite; its efficacy control (I) and control K are BROKEN (R3-B2/B3). W-SM-7 PASS; W-SM-14 default safe.

## Synthetic motion (R2-09)
No pointer/relative events during any pan scenario (U05, U05b); F17 (restore_focus at pan end) detected. Matches the recorded owner decision.
