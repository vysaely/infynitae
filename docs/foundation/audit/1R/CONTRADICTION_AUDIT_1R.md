# Phase D checkpoint 1R — focused contradiction audit

**Scope:** pairwise interactions among the 1R corrections and the areas named by the authorization:
- Camera authority, viewport-fit, zoomed pan, scale epochs;
- popup grabs/focus, pointer lock/confine, output removal;
- fullscreen/maximize, activation, frame kick/fallback.

**Rules applied:** FOUNDATION_ARCHITECTURE §C12 as amended by §C12.R1.

| # | Pair | Analysis | Result |
|---|---|---|---|
| 1 | Camera authority × viewport-fit | Fit is an authorized one-shot action consuming one proof. No mode remains that a client could feed (ER-4: probes 2d/2e, 200-commit test). | consistent |
| 2 | Camera authority × shell IPC | `Navigate` and `FitViewport` are atomic shell commands, one proof each. Without animation, nothing continues after the command returns, so shell disconnect leaves nothing running (C7.3 "long-running effect" superseded). | consistent; C7.3/AUD-6 superseded (C12.R1 table) |
| 3 | Camera authority × activation | Activation yields focus, attention or nothing. A focus grant changes ActiveElementFocus, never a camera (no proof path in `core::protocol`). | consistent |
| 4 | Viewport-fit × zoomed pan | Fit leaves zoom ≠ 1; a later pan uses the corrected algebra. Its visual shape jitter is the stated OB-24 residual, not a contradiction. | consistent (residual stated) |
| 5 | Viewport-fit × scale epochs | A fit is an atomic one-step epoch that settles once. The client's later resize cannot move the camera, so ρ changes only through client geometry under P7/SC-E. | consistent (ER-5 R3: 1 change for every client class) |
| 6 | Viewport-fit × fullscreen | A fit is a camera mutation and fullscreen a presentation; precedence is unchanged (the presentation renders on top). A fit during fullscreen changes camera O, which shows after exit. | consistent |
| 7 | Zoomed pan × scale epochs | A continuous pan opens an epoch; increases only at rung crossings; settles at release. | consistent |
| 8 | Scale epochs × kick/fallback | The epoch deadline is a compositor timer armed only while an epoch is open, an obligation per C14.2 (INV-IDLE-1). Scale events are not configures and not kick triggers. Unobserved elements hold h (S7), and settlement with ρ undefined keeps the held value. | consistent |
| 9 | Scale epochs × output removal | Removal is a settlement event: it closes the epoch and evaluates once on the new ladder. Even with a gesture still physically held, one change at most, then the orphaned pan mutates nothing. | consistent |
| 10 | Scale epochs × SC-E | Inside an epoch INV-SCALE-3 governs; outside, SC-E (INV-SCALE-4(d)). P7 fixes the direction down in both. | consistent (ER-5 R2 client case) |
| 11 | Popup grabs × ActiveElementFocus | A grab routes only within the focused element's tree and never changes AEF. A core AEF change preempts in the INV-INPUT-10 order. | consistent (ER-1) |
| 12 | Popup grabs × compositor pan acquisition | C4.6 already dismissed the grab chain before a pan. 1R fixes the order (ungrab → keyboard → pointer). Acquisition does not change AEF, so no `set_focus` follows. | consistent |
| 13 | Popup grabs × kick | `popup_done` is a kick trigger, so preempting a grab of an unobserved focused client kicks first. | consistent |
| 14 | Popup grabs × pointer lock | A grab request is accepted only with a valid input serial, normally from a click (a client-owned sequence). A lock on the same element stays per INV-INPUT-5. Reconciliation is deferred during the client sequence. | consistent |
| 15 | Pointer lock × ActiveElementFocus | **Closed in 1R.** INV-INPUT-5 now requires AEF = the constraint's element for activation and keeping. An activation grant to another element is a reconciliation trigger, so a game's lock does not survive the user focusing another app. | consistent (rule added) |
| 16 | Pointer lock × output removal × client grab | Relocation is immediate; the constraint is deferred until the client sequence ends; exactly one `unlocked`. | consistent (ER-2) |
| 17 | Pointer lock × camera motion | Compositor pan: deferred (KeepFocus; X-8). Atomic or shell change: batch-end reconciliation through the interaction output. | consistent |
| 18 | Confinement × output removal | Relocation is a forced crossing. Batch-end reconciliation: a pointer outside the region gives `unconfined`; inside, the clamp continues on the new interaction output. | consistent |
| 19 | Confinement × Arrangement | Crossing is disabled while confined (ER-5 R5). | consistent |
| 20 | Fullscreen × maximize | Maximize is declined with a configure and never fullscreens. Fullscreen policy is independent. Both answer every request. | consistent (ER-3) |
| 21 | Fullscreen × satellite (pre-map request) | Before the first map the request is granted in the initial configure (EC-13 path unchanged). | consistent |
| 22 | Fullscreen × focus | The grant needs AEF. Losing AEF later does not end fullscreen. A newer grant on the same output replaces the old one, with a configure to the old element. | consistent |
| 23 | Fullscreen × pointer | Presentation start/end on the interaction output is a reconciliation trigger. | consistent |
| 24 | Maximize × camera / Stage | No camera path (no proof in protocol code); Stage untouched (ER-3). | consistent |
| 25 | `activated` × camera | `activated` changes only on AEF transitions, which camera, visibility and pointer motion never cause, so INV-CAMERA-4 still holds. | consistent |
| 26 | `activated` × kick | An `activated` configure to an unobserved element is a non-initial configure, a kick trigger, at most one per focus transition. | consistent |
| 27 | Kick/fallback × deferred constraints | No interaction: a constraint needs an observable surface through the interaction output, and kicks concern unobserved clients. | consistent |
| 28 | Initial hint × epochs | Map is a non-client stimulus. The initial h equals the first evaluation, so 0 renegotiations at map. | consistent |
| 29 | One-shot fit × D5 epoch 1 zoom origin | The zoom came from a fit, then navigation; legal under the 1R rules. The pan uses the corrected algebra. | consistent |
| 30 | INV-SCALE-2 off-ladder held hint × emission | Only emitted values must be rungs; a held value is never emitted until re-evaluated. | consistent |

**New authority cycles:** none.
- The only remaining client-influenced loop is h ↔ client logical size. It is bounded by P7 + epoch + SC-E and touches no authority.

**New Level-A contradictions:** none.

**Sealed text superseded by 1R** (listed normatively in C12.R1):
- C5.1 (re-fit, isolation, restore-on-unmap, "user navigates ends fit");
- C5.2 (deferred unfocused request);
- C7.3 (long-running animated navigation);
- C14.4 SC-D;
- B11.1 / B11.4 (pan formula, reconciliation only at termination);
- AUD-6, AUD-12/13 wording;
- CP3A-5 cycle 2;
- C2.1/C2.2 rows (ViewportFit, PointerModel, keyboard focus);
- C4.3 activation wording;
- C4.5 keyboard-focus state name.
