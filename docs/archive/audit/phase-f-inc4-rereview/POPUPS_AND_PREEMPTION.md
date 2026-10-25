# INC-4 Re-Review — Popups, Preemption, OB-4, Zero Outputs, Rule 1

## 1. Popup authority
Infynitae still keeps its own `popup_grab: Option<PopupGrab>` beside Smithay's seat-held grab chain. The correction filters it with `popups.cleanup()` + `has_ended()` and guards unsets with `has_grab(g.serial())`. Divergence remains:
- **R09b:** nested grab with a distinct second serial → W-SM-14 keeps the pointer grab installed with the first serial → preemption checks only `g.serial()` (second) → **step 3 skipped; stale `PopupPointerGrab` survives** the focus change (R09 passes only when both popups reuse one serial).
- **R12:** popup grab → last output removed → reconnect: the zero-output grab replaced the popup pointer grab (Smithay `PopupPointerGrab::unset` tears down the keyboard grab), the adapter re-focuses the popup, the reconnect unsets the zero-output grab. Result: popup still "grabbing" with keyboard route P but **no keyboard or pointer grab**; a click outside no longer dismisses it.

## 2. Refused grabs / W-SM-7 — **PASS**
`popup_done` on: root not found, root not an element, root not focused, keyboard grabbed by another sequence (ATK-11, L and N detected). Smithay `Err` cases dismiss (ParentDismissed) or post protocol errors (InvalidGrab, NotTheTopmostPopup) itself (`manager.rs:111-127`). Precondition cannot panic by construction. PF-WSM7 itself remains vacuous (asserts only no keyboard violations).

## 3. W-SM-14 — **FAIL (default safe, still runtime-disableable)**
Production default `nested_grab_keep_pointer = true`; V detected (PF-6 no longer forces it). `set_nested_grab_keep_pointer` is still a public production setter (INC4R-03).

## 4. W-SM-15 / ordered preemption — **PARTIAL**
Permanent non-vacuous coverage exists: `neg2_pointer_first_teardown_control` compares ORDERED vs pointer-first client wires (I and J detected), PF-1 asserts no residual grabs (K detected), N10/N11/N12 detected. Survivors: **N09 (keyboard focus set before `ungrab`)**. Step 3 skipped for nested distinct-serial grabs (R09b). Omission of `ungrab(All)` detected (N10).

## 5. CameraPan dismissal (F05 / OB-4) — **pointer side PASS; keyboard side FAIL**
Smithay call paths: the OB-4 branch no longer calls `PointerHandle::unset_grab`. The remaining routes to `unset_grab` are (a) the ordered preemption branch (focus changes only), (b) `PointerZeroOutputEnd`, (c) Smithay's own `PopupPointerGrab` self-unset on the next pointer event once the grab has ended. Route (c) is reached at pan end: R10 shows the pan-end wire identical to a no-popup control (`motion 0,30`, `frame`) — no extra replay — and no client pointer/relative events during the pan. N07 (reintroducing the unset) detected.
Keyboard side: the OB-4 branch does `ungrab(All)` + keyboard `unset_grab` but never restores focus to the element root → **INC4R-01** (R16). N08 (keyboard unset omitted) survives.

## 6. Zero outputs (INC-3 gate) — **mostly PASS**
The failed-review ATK-12 sequence *with* the focus change, plus a nested popup, then reconnect: zero-output grab kept during preemption, removed by the first output (R11). Held keys, implicit grabs, CameraPan and repeated cycles: R17 150 cycles bounded. Defect: R12.

## 7. D-B6-BOUNDARY rule 1 — **NOT RESOLVED**
Three outputs, all cameras centred on the parent (visible on all): click (causing event, serial) on A 1920×1080/60 Hz; pointer then moved to B 800×600/60 Hz; clock output C 1280×720/144 Hz. Popup 600 wide at anchor x = 390 with slide. Expected (A) x = 360; got **x = −200 (B)** (R15). The production substitute is `Core::pointer_output()` at placement time, which is neither the grab-serial event's output nor (after motion) the seat's last *causing* input. N21 (wiring removed) survives; the rule-1 selection efficacy control was retired.
