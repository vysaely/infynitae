# Checkpoint

CURRENT_PHASE: **PHASE F — INC-6 RATIFIED AND INTEGRATED INTO LOCAL `main` (`ca2e468`, fast-forward authorized 2026-10-17); NB-1 FIX INTEGRATED INTO LOCAL `main` (`1f2b246`, 2026-10-17); LAYER C12.F14 (OB-24 decision) INTEGRATED INTO LOCAL `main` (`270128f`, fast-forward authorized 2026-10-17); LAYER C12.F15 (canonical render/hit rect; F-FIT-1) ADOPTED ON LOCAL `main` 2026-10-17; LAYER C12.F16 (bounded fit/restore history, 256 per output) ADOPTED ON LOCAL `main` 2026-10-18; INC-10 IMPLEMENTED ON LOCAL `main` 2026-10-18 (exit evidence produced, not accepted); INC-10 EXIT-REVIEW FINDING M1 FIXED ON LOCAL `main` 2026-10-18; LAYER C12.F17 (INC-10 exit clarifications; owner decisions Q1–Q5) ADOPTED 2026-10-20 (base `16f13c9`); INC-10 (zoomed navigation) RATIFIED 2026-10-20 (ratified commit `faf6dda`). NOT PUSHED: `origin/main` is `23752f1`.** INC-0 … INC-7 are RATIFIED and on local `main`; INC-6 amendments C12.F11–C12.F13; OB-13, OB-28, OB-42 CLOSED. The Core visibility remediation (`85a74cb`) is integrated and FORMALLY RATIFIED. The runnable prototype runtime remains EXPERIMENTAL pending INC-13. **OB-24 is decided by the owner (2026-10-17), recorded as layer C12.F14, on local `main` since `270128f`: G-OB24 (as amended by C12.F14.5) is met. OB-24 (its closure awaits change control on the INC-10 change-control record's U-1 and U-8), OB-44 (C14 baselines), OB-52 (real renderer limit) and OB-53 (INC-10 part met; later scale stimuli) remain OPEN; OB-14 and OB-54 … OB-59 are OPEN. INC-10 is RATIFIED (2026-10-20).**

Date: 2026-10-21

## PHASE F — SUPER+DELETE CLOSES THE FOCUSED WINDOW (branch `feature/close-binding-2026-10-21`; not pushed)
- **Authority:** the owner's interaction intent of 2026-10-05, confirmed 2026-10-21 (evidence/phase-f/KEYBINDINGS_INTENT_2026-10-21.md, which also lists the intended bindings and their conflicts with the code: Super+Enter).
- **Implemented:** `KeyAction::CloseFocused`, `Effect::CloseRequest` → `xdg_toplevel.close`; T0 `logo_delete_closes_with_exact_modifiers`, T1 `t1_close_binding`; 2 seeded controls.

## PHASE F — LAYER SURFACES IMPLEMENTED ON BRANCH `feature/layer-shell-2026-10-21` (2026-10-21; owner decisions L1–L8; not `main`, not pushed)
- **Implemented:** `zwlr_layer_shell_v1` — Core's layer surfaces (arrangement, exclusive zones and the usable area for placement and fit, hit order, keyboard interactivity, presentation hiding, output removal), the adapter's handler, drawing above/below the Canvas, frame callbacks.
- **Evidence:** evidence/phase-f/LAYER/IMPLEMENTATION_RECORD_2026-10-21.md (7 T0, 5 T1 with a real layer-shell client, 9 seeded controls). Prepared, not adopted: C12.F19. Not done: layer popups, constraints on layer surfaces.

## PHASE F — OWNER DECISIONS 2026-10-21; INC-11 AND INC-12 ON LOCAL `main` (`839f287`; not pushed)
- **Integrated:** owner fast-forward of local `main` to `839f287`: INC-12 (EXPERIMENTAL, not ratified) and INC-11 (implemented, not accepted, not ratified).
- **Decisions:** OB-40 Strict (OB-40 stays OPEN, U-8), decorations none (xdg-decoration to come), the shell is Quickshell-class (layer-shell next) — evidence/phase-f/OWNER_DECISIONS_2026-10-21.md.
- **Fixes (flaw scan F5, F9):** one shell stream at a time; no panic in `confined_motion`.

## PHASE F — INC-11 IMPLEMENTED ON BRANCH `feature/inc11-shell-ipc-2026-10-20` (2026-10-20; from `integration/inc12-on-main-2026-10-20`; NOT RATIFIED; not `main`, not pushed)
- **Authority:** owner decisions 2026-10-20 D1–D10 (evidence/phase-f/INC-11/DECISION_RECORD_2026-10-20.md).
- **Implemented:** the shell commands in Core (`shell_command`, shell streams, the instance per run, toplevel metadata); `infynitae-ipc` (JSON lines, Snapshot/Subscribe, generations); the binary's shell socket with SO_PEERCRED admission; the loop extension.
- **Evidence:** evidence/phase-f/INC-11/IMPLEMENTATION_RECORD_2026-10-20.md (exit tests, OB-35, OB-53's INC-11 part, 13 seeded controls). Not accepted.
- **Prepared, not adopted:** layer C12.F18 (evidence/phase-f/INC-11/C12F18_PACKAGE_2026-10-20.md). **D4 / OB-24 U-1 (b): decided, not applied** (needs U-8's checker change).

## PHASE F — INC-12 INTEGRATED ON BRANCH `integration/inc12-on-main-2026-10-20` (2026-10-20; from `main` `a058657`; EXPERIMENTAL, NOT RATIFIED; not `main`, not pushed)
- **Integrated:** the xwayland-satellite supervisor and xdg-activation (`fix/inc-12-satellite-runtime-dir-2026-10-16`, merge `a7d9bd7`) and review finding R2-4 (`eaf1adb`: the satellite is an owned child).
- **Gates on `eaf1adb`:** all PASS — tests 610/0/4, seeds 452, efficacy 450/450 DETECTED, envverify, NB-1.
- **Exit criteria:** 1 and 4 met; OB-40 (activation policy), the real-satellite and operator runs, and the closure of OB-9/OB-27/OB-33 parts remain (evidence/phase-f/INC-12/INTEGRATION_ON_MAIN_2026-10-20.md).

## PHASE F — ERRATUM: COMMIT HASHES REWRITTEN FOR E-MAIL PRIVACY (2026-10-20; owner decision B; not pushed)
- **Authority:** owner decision B, 2026-10-20: a personal e-mail address recorded as author and committer of local commits since 2026-10-16 is replaced by `vysaely <vysaely@users.noreply.github.com>` before the first push.
- **Effect:** commit metadata only. 121 commit hashes changed (75 on `main`); every tree is byte-identical; `origin/main` `23752f1` and the historical seals are unchanged.
- **Reading recorded hashes:** the hashes written in this file and in the other foundation documents, the evidence and `tools/check-foundation-docs` are the pre-rewrite names and are not edited; the map is evidence/REWRITE_HASH_MAP_2026-10-20.md (e.g. INC-10 ratified commit `faf6dda` → `4d5be84`; INC-6 ratified commit `ca2e468` → `da062bf`).
- **Declaration:** no ratified, adopted or normative text changed.

## PHASE F — INC-10 FORMAL RATIFICATION (zoomed navigation; 2026-10-20; ratified commit `faf6dda`; not pushed)
- **Authority:** owner authorization 2026-10-20: INC-10's exit evidence accepted through architecture change control, with the decisions in the record, and INC-10 ratified.
- **Ratified commit:** `faf6dda` (local `main` `56b3f12` + `b594900` + `a56cc3c` + `632873c` + `c3fa23b` + `eca016b` + `d678d32` + `faf6dda`).
- **Verification:** change-control record `evidence/phase-f/INC-10/CHANGE_CONTROL_RECORD_2026-10-20.md` (gates on `56b3f12` and on `faf6dda`; exit tests and items 1–11; obligations; independent review and re-check).
- **Scope:** F-FIT-1 / OBJ-30 fit and restore (C12.F15, C12.F16, C12.F17); zoomed pan with the kept lattice reference (C12.F14); canonical-rect wrapper, crop, tracker rebuild (C12.F15, C12.F17); bindings (M8); pointer reconciliation (INV-INPUT-5, M10). Software renderers, headless.
- **Obligation dispositions:** OB-24 OPEN (closure awaits U-1 and U-8 of the change-control record); OB-53 INC-10 part met, OPEN; OB-14 OPEN; OB-54 … OB-59 OPEN.
- **Record:** evidence/phase-f/INC-10/RATIFICATION_RECORD_2026-10-20.md.
- **Declaration:** **INC-10 is RATIFIED.** Not pushed: `origin/main` is `23752f1` until the owner pushes.

## PHASE F — HYGIENE AND KNOWN LIMITATIONS (2026-10-20; branch `prep/inc10-change-control-2026-10-20` from local `main` `56b3f12`; not `main`, not pushed)
- **Authority:** owner authorization 2026-10-20 (session brief item 5): a kill-and-reap guard in `t1_tiny_compositor`'s session-outlive test; U-006's teardown behaviour noted here as a known limitation.
- **Test hygiene:** `the_session_command_does_not_outlive_the_session` holds a guard that kills and reaps the session command when the test ends with it still present, however the test ends (a failure, a panic or the seeded control "session: children are not terminated at shutdown", which stays DETECTED). Before it, a failing run left its `sleep 30` behind (session report 2026-10-20 §3).
- **Known limitation (U-006, `crates/infynitae-wayland/src/signals.rs`):** the SIGTERM/SIGINT/SIGCHLD handler is installed once per process and never restored; after the last registration ends it does nothing, so in a process that hosted a session SIGTERM and SIGINT stay **ignored** while no session is registered (a test binary is not stopped by Ctrl-C between sessions; the compositor binary hosts one session and exits with it). Each registration leaks its write end (one descriptor per registration, two or three per session, never reclaimed; at most 16 live registrations per signal) — U-005. By design for an orderly teardown (`repeated_signals_shut_down_once`); accepted, not a defect; revisit if a process ever hosts sessions repeatedly (e.g. an embedding or a long-lived test harness).

## PHASE F — LAYER C12.F17 ADOPTED 2026-10-20 (base `16f13c9`, branch `integration/owner-decisions-2026-10-19`; not pushed)
- **Authority:** owner decisions of 2026-10-19 — Q1 B scope 2, Q2 widen, Q3 exactly one, Q4 forward, Q5 defer to OB-57; OB-14 and OB-53 dispositions; OB-54 workaround kept with W-SM-21; drag-and-drop refused, OB-59 — and the owner's adoption of the package (`evidence/phase-f/C12-F17/C12F17_AMENDMENT_PACKAGE_2026-10-18.md`, §8–§9).
- **Layer C12.F17:** INV-INPUT-5 (no absolute motion to a locked surface on any path; one true-position event after a camera action, a reconfiguration, an interaction's end and a relocation unless a lock is kept); F-RENDER-1 (INC-10 status, crop sampling and bounds, damage); F-FIT-1 (refused while O presents; reconciliation under a constraint); M8 (binding rows; presentation guards); M10 (interaction end, reconfiguration, relocation); ADR-04, ADR-07; INC-10 exit items 4 and 6 and its obligations; W-SM-21; OB-59; History of OB-14, OB-53 … OB-57. No invariant added or removed (57). Checker: `F_LAYERS`, fixtures 47–48.
- **Declaration:** INC-10 is **NOT ratified**; OB-24 and every other OB stay **OPEN**. Not pushed: `origin/main` is `23752f1`.

## PHASE F — OWNER DECISIONS OF 2026-10-19 INTEGRATED ON BRANCH `integration/owner-decisions-2026-10-19` (from local `main` `619187f`; NOT `main`, not pushed)
- **Authority:** owner decisions of 2026-10-19 (session report 2026-10-18 §6, answered): merge the defect fixes after independent review; the OB-55 fix (C12.F17 Q2 widened); integrate INC-8 (Stage 1 rehearsal `ca7b5d5`); the clipboard (drag-and-drop stays refused, OB-59 proposed in C12.F17.11); the orderly-shutdown and process-ownership design with the self-pipe on `libc` (`signal-hook` rejected; UNSAFE_INVENTORY U-005/U-006). Fast-forwarding `main` to this branch is the owner's decision.
- **Order (full gates after each step):** `fix/defect-verification-2026-10-18` (Q-B1, OB-54 workaround, Q-P1, `--check-seeds`) → `prep/ob55-q2-gated-2026-10-18` → INC-8 (`ca7b5d5`) → the C12.F17 package with the Q1–Q5 decisions and their code (Q1 B scope 2: fit and restore refused while O presents; Q3: exactly one true-position event at an interaction's end and a relocation unless a lock is kept) → clipboard → orderly shutdown. Gate logs: the session report.
- **INC-8 resolution (from the Stage 1 rehearsal):** INC-8's frame-liveness hooks re-expressed on `main`'s caller-owned-buffer API (C14.3, OB-42): `Core::frame_tick_into` records completions (now `&mut self`), the commit hook sits in `toplevel_committed_acked_into` (success only); kick, fallback and their timer unchanged; the session sleeps until the earliest of the frame, the run limit, Core's settlement deadline and the fallback.
- **Declaration:** C12.F17 is **NOT adopted** here (its adoption commit is prepared separately; the owner adopts it). INC-10 is NOT ratified; OB-24 and every other OB stay OPEN (OB-16 included). Not pushed: `origin/main` is `23752f1`.

## PHASE F — INC-8 RATIFIED, T1 SCOPE (2026-10-16; governance branch `governance/inc-8-ratification-2026-10-16`; not merged, not pushed)
- **Authority:** owner decision 2026-10-16 (brief "Ratification, Clipboard, Orderly Shutdown and First Nested Runtime", Phase 2): "Ratify only within the defined T1 scope and record its remaining limitations. Keep OB-16 open until the required real-application evidence is collected."
- **Ratified commit:** `375b44f` (`experimental/inc-8-offscreen-frames`): implementation `60c05a1` … `a45b0f0`, review fix `bdeb219` (a popup-grab dismissal kicks the grab's owner or no one), review test `b110398`, ratification review `375b44f`.
- **Scope (T1, headless):** M16 — withholding for unobserved elements, the kick before input, non-initial configures, keyboard and pointer `leave` and `popup_done`, the fallback timer armed only with `frame_requested` (PROV-4 P = 1 s, provisional), re-entry at the first vblank; OBJ-17 FrameLiveness; INV-FRAME-1…5 and INV-IDLE-1 as exercised. INV-FRAME-6: no suspension reason is producible (vacuous, as planned).
- **Independent verification before recording (2026-10-16):** exit tests TST-03 SC3 and TST-06 x9 (`t1_frames`, `t1_outputs`), TST-11 T1 real-time harness (`t1_frames_realtime`), hidden-by-presentation re-entry (`t1_inc7_adversarial`), Core frames unit tests: all PASS on `375b44f`; negative controls "pure starve" and "no kick before input": DETECTED; gates at `375b44f`: 374 tests, 251 seeded defects detected, all PASS. Combined behaviour with INC-6 checked on the integration line (`a_hidden_windows_lowered_hint_releases_no_withheld_callback`).
- **Obligation dispositions:** OB-16 **T1 part met** (first client response within one frame of the input: 86 µs median, 131 µs max over 50 runs, headless real time); OB-16 stays **OPEN** (T3: real applications). OB-5 OPEN (T5).
- **Limitations recorded (not part of the ratified scope):** no T3 or hardware evidence; no T4 idle or CPU measurement (OB-44); residuals F-1 (Smithay's own outside-click `popup_done` bypasses the kick), F-2 (completion stamped at the next evaluation), F-3 (`frame_kick` cost not measured), F-4 (PROV-4 provisional). Record: evidence/phase-f/INC-8/RATIFICATION_REVIEW_2026-10-16.md.
- **Integration:** not done. INC-8 is independent of INC-6 by plan; whichever lands first, the other rebases (resolutions in `rehearsal/inc-8-on-inc-6-candidate-2026-10-16` and the integration line). `main` changes only on the owner's approval.


## PHASE F — INC-8 OFF-SCREEN FRAME POLICY — EXPERIMENTAL, superseded by the ratification entry above (record: evidence/phase-f/INC-8/)
- **Branch:** `experimental/inc-8-offscreen-frames` from canonical main `23752f1`; not merged, not pushed. Implemented 2026-10-16 under the owner's continuation brief (autonomous local work on branches). INC-8 depends on INC-2 only; no INC-6 material is on this branch.
- **Scope:** M16 — withhold, kick (non-initial configure, `popup_done`, input incl. focus enter/leave; OB-16 CR-7), fallback once per P (PROV-4 candidate 1 s) on a timer armed only while an unobserved element has a pending callback (INV-IDLE-1), re-entry at the first vblank (INV-FRAME-4); OBJ-17 in Core; OBJ-05 `frame_requested`. M17 suspension: no reason producible, nothing implemented.
- **Residual R-10 (INC-7):** the placeholder test is replaced by its INC-8 acceptance test (a window hidden by a presentation gets the fallback).
- **Tests:** 7 Core, 13 T1 (`t1_frames`), 1 real-time T1 harness (`t1_frames_realtime`, TST-11 T1 part: a FIFO-blocked off-screen client answers a key in 86 µs median, max 131 µs over 50 runs; kick disabled ≈ 700 ms), 1 replaced T1, 1 session; 15 seeded controls.
- **Not met / not claimed:** OB-16 stays OPEN (T3 part; review); OB-5 (T5); any nested, GPU or real-application result; independent review.
- **Declaration:** INC-8 is EXPERIMENTAL and NOT ratified; INC-8 exit is NOT authorized. INC-6 is unchanged (NOT ratified, exit NOT authorized). OB-13, OB-16, OB-22, OB-42, OB-44 and OB-52 are OPEN.

## PHASE F — INC-10 EXIT-REVIEW M1 FIXED; C12.F17 PACKAGE PREPARED (2026-10-18; local `main`, base `c4a7263`; not pushed)
- **Authority:** owner brief "INC-10 M1 Resolution and C12.F17 Change Control" (2026-10-18), after the independent read-only exit review of `c4a7263` (READY AFTER SPECIFIED GOVERNANCE CLARIFICATIONS; M1 medium; L1, L2, L4, L5, L6). Record: evidence/phase-f/INC-10/M1_RESOLUTION_RECORD_2026-10-18.md.
- **M1 fixed:** after a fit or restore the true-position event is suppressed only by a lock the reconciliation keeps (INV-INPUT-5; no motion to a locked surface); a kept confinement gets exactly one event, as F-FIT-1 requires. T1 `t1_fit_constraints` (none / confined / locked / confinement ended / pan-owned) with two seeded controls.
- **Also in the implementation commits:** exact modifier matching for Logo+F and Logo+Shift+F (Ctrl or Alt held: no binding); T1 binding-eligibility tests (no target, pan, presentation as ratified by §C5); the review's pixel sweep (153 remapped frames, 0 wrong pixels), crop E1 (viewport source, buffer scale, 8 transforms) and crop-box E3 in the maintained suite; L1 (no per-frame `OutputModel` clone: 9 → 5 adapter allocations per static frame) and L2 (`verify-test-efficacy`: a featured build clears no stale mark). Seeded controls 380 → 388, each required to be DETECTED. Gates: run on the final tree (results in the commit messages).
- **Follow-ups (OPEN obligations):** OB-54 constraint lifetime reported as persistent by the adapter (oneshot locks re-activated); OB-55 absolute motion to a locked surface at `output_reconfigured`; OB-56 pan release under a kept confinement, and re-activation of a lock whose point is unobservable (decision); OB-57 GPU render evidence (GLES, fractional crop cuts, scanout, pacing); OB-58 frame-time method and adapter per-frame allocations (L6).
- **C12.F17:** the governance clarifications the review asked for are prepared as a decision package for change control — evidence/phase-f/C12-F17/C12F17_AMENDMENT_PACKAGE_2026-10-18.md. **Not adopted; no living or historical text is changed.** Open owner decision Q1: fit/restore during a protocol-fullscreen presentation (ratified §C5 lets a fit change the hidden camera; the brief asks for no silent change).
- **Declaration:** INC-10 is **not ratified**; its exit evidence still awaits acceptance through architecture change control. **OB-24 remains OPEN.** Not pushed: `origin/main` is `23752f1`.

## PHASE F — INC-10 IMPLEMENTED ON LOCAL `main` (2026-10-18; exit evidence produced, NOT accepted; not pushed)
- **Authority:** owner brief "INC-10 Production Implementation and Evidence" (2026-10-18). Record: evidence/phase-f/INC-10/IMPLEMENTATION_RECORD_2026-10-18.md.
- **Commits (base `e75ea72`):** `b46021f` the output's kept lattice reference and zoomed pan; `2884287` the `CanonicalRect` wrapper, the i32 crop for all buffer transforms, the tracker rebuild on a k change; `cdf3a09` the one-shot fit and the bounded restore history (256 per bound camera); `73c0aca` lifecycle while zoomed and INV-ALLOC-1 at zoom ≠ 1; the evidence commit (this section).
- **Exit evidence (plan INC-10 items 1–11):** produced on the production path with Smithay 0.7.0's damage tracker and its software renderer — render/hit sweep 139,392 rects with 0 presented mismatches, 0 seams and 0 wrong pointer pixels; crop drawn correctly for all eight buffer transforms; fit/restore and history tests; lifecycle; frame times (Pixman and no-op renderer) and zero hot-path allocations at zoom 0.73. Seeded controls: 380, each required to be DETECTED. The 200-commit "client resize never moves the camera" test did not exist and is added. Gates: run on this commit's tree (results in its commit message).
- **For change control (ratified text not edited):** (1) C12.F15.5's named `CropRenderElement` formula misdraws the four rotating buffer transforms; the production crop follows the renderers' sampling map; (2) the box `[−2³⁰, 2³⁰]` spans one pixel more than an i32 size, so a whole-box span stops at 2³⁰ − 1; (3) C12.F15.4's "exactly": pass-through damage is Smithay's, rounded outwards by ≤ 1 px; (4) the fit target and the bindings (Logo+F / Logo+Shift+F) are Level D defaults, not owner decisions.
- **Gate tool fix:** `tools/verify-test-efficacy` reused a crate built from an earlier control's seeded source in later controls (shared target directory, copies keeping old modification times; a stale seed made one INC-10 control pass in two full runs). Every seeded file now gets a fresh time in later copies until its crate is rebuilt clean. Earlier full-run verdicts predate this fix.
- **Not established:** GLES or any GPU path, dmabuf, direct scanout, hardware planes, presentation pacing.
- **Declaration:** INC-10 is implemented, **not ratified**; its exit evidence awaits independent review and acceptance through architecture change control. **OB-24 remains OPEN.** Not pushed: `origin/main` is `23752f1`.

## PHASE F — LAYER C12.F16 ADOPTED 2026-10-18 (local `main`, base `da538d6`; not pushed)
- **Authority:** owner brief "Ratify Bounded Fit/Restore History" (2026-10-18). Record: evidence/phase-f/C12-F16/DECISION_RECORD_2026-10-18.md.
- **Layer C12.F16:** INV-CAMERA-5 and F-FIT-1 — the fit/restore history (OBJ-30) holds at most 256 entries per output, one per successful fit not yet restored; a successful fit at capacity evicts the oldest (lost permanently); entries are exact prior camera values (Q16 centre, `f64` zoom) restored verbatim, newest first; a refused fit or restore changes nothing; the history belongs to a camera's binding to one `OutputId`, is emptied when `output_disconnected` unbinds the camera and kept across `output_reconfigured`, so it is never applied to another output or a later connection; fixed storage, none for an unbound camera. OBJ-30, M5, M8, ADR-04, INC-10 (entry condition met; exit item 11). No invariant added or removed (57). Checker: `F_LAYERS`, fixture 46.
- **Not implemented:** governance only; the history, fit and restore are INC-10's, with the tests of C12.F16.6.
- **Declaration:** this decides the depth bound C12.F15 left open. OB-24 remains **OPEN**. INC-10 is **NOT started**. Not pushed: `origin/main` is `23752f1`.

## PHASE F — LAYER C12.F15 ADOPTED 2026-10-17 (local `main`, base `dda4d87`; not pushed)
- **Authority:** owner brief "Ratify C12.F15 and Record C12.F14 Integration" (2026-10-17), decisions 2–5. Record: evidence/phase-f/C12-F15/DECISION_RECORD_2026-10-17.md.
- **Layer C12.F15:** INV-RENDER-1 and F-RENDER-1 (option B: Core's F-LATTICE-1 rect is the one canonical rect; the adapter wraps Smithay's element so that its geometry and draw destination are that rect; conservative damage/opaque remapping; crop to ±2³⁰ before i32; tracker rebuilt on any k change); F-FIT-1 (`zoom = clamp(min(view_w,O / g.w, view_h,O / g.h) / scale_O, 2⁻²⁰, 2¹⁶)`, centre clamped into INV-COORD-1, degenerate or unmapped W refused); OBJ-30 (restore history); M8; ADR-02, ADR-04; errata to C12.F14 (left edges; the fit reaches the upper bound); Q16 budget 3·2⁻²² < δ. No invariant added or removed (57). Checker: `F_LAYERS`, E1R unchanged-list scope, fixtures 44–45.
- **Not implemented:** governance only; the wrapper, crop, tracker invalidation, fit and restore are INC-10's. Evidence: a prototype in a scratch export (pixels, sweeps, suite), stored as evidence, not as code.
- **Open:** rotated/flipped-buffer crop unverified (INC-10 exit requirement); the history's depth bound (owner decision before restore ships).
- **Declaration:** OB-24 remains **OPEN**. INC-10 is **NOT started**. Not pushed: `origin/main` is `23752f1`.

## PHASE F — LAYER C12.F14 INTEGRATED INTO LOCAL `main` (2026-10-17; not pushed)
- **Authority:** owner brief "Integrate C12.F14, Then Prepare C12.F15" (2026-10-17), Phase 1: fast-forward local `main` to the reviewed C12.F14 commit `270128f` after the repository-state and ancestry checks; no push. Recorded under the owner brief "Ratify C12.F15 and Record C12.F14 Integration" (2026-10-17), decision 1.
- **Integrated:** `governance/ob24-c12f14-2026-10-17` at `270128f` (`b251a43` layer, `387888e` merge of `main` `7ddde8f`, `1d5931d` control re-anchor, `270128f` corrections). Local `main` moved `7ddde8f` → `270128f` by `git merge --ff-only` (reflog: "merge 270128f: Fast-forward"). Checked before the move: `7ddde8f` an ancestor of `270128f` (4 commits); primary checkout and the governance worktree clean; 83 worktrees and the experimental branches untouched.
- **Gate evidence (executed before integration, not re-run for this record):** the full CI set plus `verify-development-environment` on tree `8137291c09d78754748a16005240fe955ca49bb6`, which is the tree of `270128f` — docs, docs self-test, knowledge, knowledge self-test, architecture, fmt, check, build, test (454 passed, 0 failed, 1 ignored: the NB-1 runtime check needing `unshare`, mpv and Mesa), clippy, `verify-test-efficacy` PASS, `verify-development-environment` ALL CHECKS PASSED. Logs outside the repository: `~/.cache/infynitae-integration/ob24gov-logs/r2/`. An independent read-only review of `270128f` (2026-10-17) reproduced the committed render/hit probe output and recommended conditional approval; its two errata are recorded by C12.F15.
- **G-OB24:** met — its prerequisite as amended by C12.F14.5 (the recorded architecture decision, C12.F14 on `main`) holds. **OB-24 stays OPEN** until INC-10's implementation and its exit evidence are accepted through architecture change control; meeting the gate is not closing the obligation.
- **Declaration:** C12.F14 is integrated into local `main` only. **Not pushed:** `origin/main` is `23752f1`. INC-10 is NOT started.

## PHASE F — NB-1 FIX INTEGRATED INTO LOCAL `main` (2026-10-17; not pushed)
- **Authority:** owner brief "Integrate NB-1 Fix and Finalize OB-24 Decisions" (2026-10-17) §2: integrate the reviewed host-frame-gating fix into local `main` if the ancestry and diff checks confirm only approved changes and their necessary dependencies. No push.
- **Integrated:** `fix/nested-host-frame-gating-2026-10-17` at `7d7ac73` (`e5e4823` fix, `d655f83` evidence + seeded control, `7d7ac73` regression test + isolated evidence), parent `ca2e468` — merge `1f2b246`. Scope checked: code changes only in `crates/infynitae-backend-winit/src/lib.rs` (`HostFrameGate`, one T0 test) and `tools/verify-test-efficacy` (one control); new ignored runtime test `crates/infynitae/tests/nested_hidden_window.rs`; evidence under `evidence/phase-f/runtime-validation/NB1/`. No INC-8, INC-9, INC-12, clipboard, shutdown, signal-handling or process-ownership code (the r1/r2 lines appear only as evidence of the field stalls).
- **Fix:** the nested (winit) backend submits a frame only after the host's frame callback for the previous one (`WinitEvent::Redraw`); while the host withholds callbacks (hidden or minimised window) a due frame waits and the loop keeps dispatching clients, run limits and signals. Swap interval and renderer unchanged; no unsafe code, no polling (a waiting frame re-arms no timer).
- **Evidence:** isolated host nesting, 3 runs per cell (no statistical claim): hidden, 8 s limit — unfixed 3/3 still running at 30 s, fixed 3/3 exit at 8.56 s; SIGTERM while hidden (signal-handling line r2 only) — unfixed 3/3 running, fixed 3/3 exit at 6.55–6.56 s; hidden then shown — 3/3 resume (222–223 frames after the reveal). Regression test: fails on unfixed `main`, passes on the fix. Record: evidence/phase-f/runtime-validation/NB1/NB1_INVESTIGATION_2026-10-17.md.
- **Gates:** on `7d7ac73` before the merge and on the merged `main` (CI set + `verify-development-environment`); results in the session report.
- **Declaration:** NB-1 is fixed on local `main` for the nested backend. Not pushed. The runtime remains EXPERIMENTAL pending INC-13.

## PHASE F — OB-24 DECIDED; LAYER C12.F14 ADOPTED 2026-10-17 (governance branch `governance/ob24-c12f14-2026-10-17`, base `ca2e468`; not on `main`, not pushed)
- **Authority:** owner brief "Integrate NB-1 Fix and Finalize OB-24 Decisions" (2026-10-17): record decisions D1–D6 and prepare the amendments. Record: evidence/phase-f/OB-24/DECISION_RECORD_2026-10-17.md.
- **Layer C12.F14:** P1 amended (camera zoom bounded to the representable domain 2⁻²⁰ … 2¹⁶); F-VIEW-1 (domain: mode sides 1 … 2¹⁵, scale 0.25 … 8, zoom 2⁻²⁰ … 2¹⁶; derived range ≤ 2³⁷); F-LATTICE-1 and INV-RENDER-1 (the output's lattice reference: set on connect, (re-)binding, a change of `k_O` or `view_O`, or beyond `|v_O − R_O|·k_O` = 2³¹; kept while panning; never persisted); M4, M5, M6, M20; OBJ-12; ADR-02. No invariant added or removed (57).
- **Corrections to the decision package:** the exactness bound is 2³¹, not 2⁵⁰ (the package's model used k with small denominators); M4 had no phrase "validates the hardware facts" — rejection rows added instead.
- **Code:** the domain (from `a156e93` / `58e1b40`) enforced where values enter Core; the kept reference is not implemented (INC-10). The ratified INC-6 test's 10⁶ zoom moved to 2¹⁶, with cases added at the domain's extremes of k.
- **Obligations:** OB-24 stays OPEN (Smithay-geometry evidence of the decided rule is INC-10's exit); OB-43 unchanged (OPEN, Level D). D6 (animated zoom) recorded as a future direction only.
- **Declaration:** G-OB24 is decided and is resolved when C12.F14 is on `main` (owner integration authorization). INC-10 is NOT started.
- **Corrections 2026-10-17 (owner brief "C12.F14 Governance Corrections", review of `1d5931d`; same branch, not on `main`, not pushed):** precision claims corrected — the 2³¹ bound is conservative and gives a rounding-step bound (product ≤ 2⁻²², sum ≤ 2⁻²¹ < δ), not exact edges; f64/exact disagreements exist inside the bound and with the per-frame reference (C12.F14.4 item 1; new deterministic T0 `kept_reference_rounding_steps_at_the_visible_edge_limit`). Render/hit agreement with a kept reference at k ∉ n/120 is an INC-10 exit requirement; Smithay-geometry probing finds 1 px right-edge differences with the current F-RENDER-1 handoff (item 3) — an open architecture question for INC-10. **G-OB24 amended (C12.F14.5, Option A):** met by C12.F14 on `main`; OB-24 stays OPEN until INC-10's exit evidence is accepted. The fit's domain rule moved to INC-10 (intended clamp, not yet normative). OBJ-12 reclassified (`R_O` history-dependent, not reconstructible); M4 guards completed. Replaced wording: C12.F14.6.
- **Declaration (corrected):** G-OB24 is **not yet met** (C12.F14 is not on `main`). OB-24 is **OPEN**. INC-10 is **NOT started**.
- **Status after integration (2026-10-17; record above, "LAYER C12.F14 INTEGRATED"):** the two declarations of this section describe the governance branch before integration and are kept as written. Since `270128f` is on local `main`, G-OB24 is met; OB-24 remains OPEN; INC-10 is NOT started.

## PHASE F — INC-6 FORMAL RATIFICATION (scale policy; 2026-10-16; governance branch `governance/inc-6-exit-adoption-2026-10-16`; not merged, not pushed)
- **Authority:** owner brief "INFYNITAE — Final Ratification Gates and Guided Runtime Readiness", §1 (2026-10-16): complete the formal ratification record on the designated governance branch if the branch is fully consistent with the adopted authority; do not merge or push.
- **Ratified commit:** `7aa2070` (adoption `f2bf0da` + ratification review `7aa2070`, on candidate r4 `eebb5c3`, from canonical main `23752f1`).
- **Ratification review:** adopted texts verified verbatim against package rev. 2; non-package wording found in `f2bf0da` and removed in `7aa2070` (the adoption record's "no other wording" corrected; remaining registry framing enumerated); P-3's scale-configuration clause verified in code (`eebb5c3`) and by two T0 tests and two seeded controls.
- **Scope:** INV-SCALE-1…5 as amended by C12.F11–F13; F-SCALE-1…3; ladder, epochs, SC-E, P7, presentation holds; device limit through the renderer capability interface; hot-path allocation contract (steady state); H-9.
- **Obligation dispositions (D7):** OB-13 CLOSED; OB-28 CLOSED; OB-42 CLOSED. OB-44, OB-52, OB-53 OPEN.
- **Gates at `7aa2070`:** 444 tests, 344 seeded defects detected, architecture, docs + self-test, knowledge: all PASS; `verify-development-environment`: ALL CHECKS PASSED.
- **Accepted residuals:** C12.F13 hold cost; geometric prediction (C12.F12); OB-13 residual (a side > `D_max` at scale 1); OB-42 steady-state scope.
- **Record:** evidence/phase-f/INC-6/RATIFICATION_RECORD_2026-10-16.md.
- **Declaration:** **INC-6 is RATIFIED.** It is **not integrated**: `main` and `origin/main` remain `23752f1` until the owner authorizes the fast-forward; push only with the owner's approval. INC-10's INC-6 entry condition is met once INC-6 is integrated; G-OB24 is still unresolved.


## PHASE F — INC-6 AMENDMENT PACKAGE ADOPTED 2026-10-16 (governance branch `governance/inc-6-exit-adoption-2026-10-16`; not merged, not pushed; INC-6 NOT RATIFIED)
- **Authority:** owner approval 2026-10-16 of INC-6 adoption package revision 2 (brief "Ratification, Clipboard, Orderly Shutdown and First Nested Runtime", Phase 1), conditional on the live texts matching the reported package and on no material contradiction. Adoption check: texts verified line by line against the code; one clause (P-3, "a change of the scale configuration … P7 applies at once") was not met by the code and was met by a code correction (`eebb5c3`), not by changing the text. Record: evidence/phase-f/INC-6/ADOPTION_RECORD_2026-10-16.md.
- **Layers adopted** (base `eebb5c3`): **C12.F11** (F-SCALE-2 `s_max(E)` counts every size Core asks for; INV-SCALE-2 restates it), **C12.F12** (the initial hint of a pre-map fullscreen grant; F-SCALE-3, INV-SCALE-5), **C12.F13** (C14.4 presentation holds; INV-SCALE-5 exception). No invariant added or removed (57).
- **C12.F9 withdrawn** (D4); **OB-28 split** into OB-28 + OB-53 (D5); **OB-42 wording** corrected, evidence unchanged and checked mechanically (D6).
- **Obligation dispositions approved for the exit (D7), not yet executed:** OB-13, OB-28 and OB-42 close with INC-6's ratification; OB-44, OB-52, OB-53 stay OPEN.
- **Checker:** `F_LAYERS` gains C12.F11–F13; fixtures 40–42 (one drift per layer); fixtures 23 and 24 re-anchored on the amended F-SCALE-2 text. Code comments and seeded-control names lose their "REHEARSAL / NOT ADOPTED" labels.
- **Declaration:** the amendments are ADOPTED. INC-6 is **NOT RATIFIED**; its ratification record is prepared (evidence/phase-f/INC-6/RATIFICATION_READINESS_2026-10-16.md) and awaits the owner. Nothing is on main.


## PHASE F — INC-7 FORMAL RATIFICATION (toplevel request policy: fullscreen, maximize, `activated`; popup robustness)
- **Ratified commit:** `99c24cd` (governance branch `governance/inc-7-exit-final`: C12.F10 and a comment-only correction on `governance/inc-7-layers-c12f5-f8` @ `80ead2a` (C12.F5–F8), from the integration candidate `93c2605`, itself from the INC-7 line `f077e55` and canonical main `ccb82c5`).
- **Ratification scope:** M2/M9 (every request answered by exactly one configure; fullscreen presentation F-PRESENT-1; maximize declined; W-SM-13 overrides), popup robustness (W-SM-6/7, W-SM-18), nested popups and xdg-shell stacking (OB-1, X-15/C12.F8), focus eligibility under a presentation (C12.F7), default placement at the first windowed presentation (C12.F5), deferred takeover (C12.F6), unmap of a fullscreen element (C12.F10), pointer event times (A-5).
- **Governance:** C12.F5, C12.F6, C12.F7, C12.F8 and C12.F10 adopted by owner approval and ratified with INC-7 (owner, 2026-10-14); C12.F9 **withdrawn** (owner, 2026-10-16): the OB-42 counting allocator moved to the headless backend's tests (2026-10-15); INV-ARCH-2 is unchanged and Core's test exception is removed; the number C12.F9 stays reserved and unused. Correction of the owner decision of 2026-10-14, which approved the exception and deferred its layer.
- **Evidence:** `~/Infynitae-INC7-EXIT-PREPARATION-2026-10-14/` (exit decision, governance consistency review, obligation closures, gate results); evidence/phase-f/INC-7/ and the C12.F5–F8 and C12.F10 amendment records.
- **Obligation dispositions (FOUNDATION_TEST_PLAN):** OB-1 CLOSED; OB-18 CLOSED; OB-32 CLOSED; OB-22 T1 answer-policy part met, **stays OPEN** (T3/T5, release blocker).
- **Exit tests:** TST-22 xmaximize on the production path (`t1_toplevel_requests`); nested popup chain parity (`ob1_nested_popup_chain_matches_what_is_presented`); record-only and deferral controls detected.
- **Accepted residuals (tracked follow-up work, not resolved defects):** INC-7 residual register R-01…R-15 (`INC7_RESIDUAL_AND_FOLLOW_UP_REGISTER.md`): retained focus on a hidden window (R-01, owner policy); W-SM-19 → INC-14 entry condition (R-07); hidden windows get no frame callbacks → INC-8 (R-10); W-SM-18 until the next Smithay pin (R-06).
- **Declaration:** **INC-7 is RATIFIED. INC-0 through INC-5 and INC-7 are RATIFIED.** INC-6 is NOT ratified, INC-6 exit is NOT authorized, and no INC-6 implementation is part of this ratification. INC-8 and INC-14 have NOT begun. OB-13, OB-22, OB-42, OB-44 and OB-52 are OPEN.

## PHASE F — INC-7 GOVERNANCE LAYERS C12.F5–C12.F8 ADOPTED 2026-10-14 (governance branch from the INC-7 integration candidate `93c2605`; not merged, not pushed)
- **Authority:** owner approval 2026-10-14 of the corrected C12.F5–F8 package and its supplementary scope (PL-21; M9; M11; INV-RENDER-2 with X-15, C3.3 and OB-1). Base stamp `93c2605` (`integration/inc-7-exit-2026-10-14`). C12.F9 (INV-ARCH-2 test-only allocator) is deferred to the INC-6 exit and is not applied.
- **C12.F5 adopted** (record: evidence/phase-f/placement-windowed-presentation/): INV-STAGE-1 (a) and F-PLACE-1 — default placement at the first windowed presentation; M1 (two rows); PL-21. Implementation `334b9b3` (P9b).
- **C12.F6 adopted** (record: evidence/phase-f/deferred-takeover/): INV-PRES-1 — a pre-map grant takes the output when its element maps; grants ordered by the time they were made; saved size, if available; M9. Implementation `b1aa758` (A-3).
- **C12.F7 adopted** (record: evidence/phase-f/focus-eligibility/): INV-INPUT-8 — no focus move to an element hidden by a presentation; retained focus when hidden (residual R-01, owner policy); no focus from fullscreen requests; M11. Implementation `0f45cfd`, `2aa8d65` (A-8).
- **C12.F8 adopted** (record: evidence/phase-f/popup-stacking/): INV-RENDER-2 — a toplevel's `xdg_popup`s newest first (xdg-shell; W-SM-20); C3.3, X-15 and the CP3A-4 OB-1 row superseded; living OB-1 item and W-SM-20 updated. Implementation `23860b3`, `d350ed2` (P10 #3).
- **Registry:** 57 invariants (12 layers: 37 C12, 6 C12.R1, 2 C12.R2, 5 C12.E1R, 0 C12.F1, 1 each C12.F2…C12.F8), 21 formulas (F-PLACE-1 amended by C12.F5), 52 obligations; docs self-test 37 fixtures (fixtures 19 and 22 re-anchored on INV-STAGE-1's latest layer).
- **No production code changed** by these layers (`git diff 93c2605 -- crates` is empty).
- **Declaration:** INC-7 is NOT ratified and NOT closed; OB-1, OB-18, OB-22 and OB-32 are unchanged (OPEN) pending the INC-7 exit review. INC-6 is NOT ratified, INC-6 exit is NOT authorized, and no INC-6 material is on this branch; OB-13, OB-42, OB-44 and OB-52 are OPEN; the runtime remains EXPERIMENTAL pending INC-13.

## PHASE F — GOVERNANCE RESOLUTION 2026-10-14: C12.F3, C12.F4, OB-13 SPLIT, STRESS-TEST REPAIR
- **Stress-test repair integrated** (`d6714f6`, fast-forward): the long-lived stress tests establish and verify their pointer target in every cycle. Before it, 26 of the 200 cycles of `round4_long_lived_combined_stress` delivered their click and the menu chain held its grabs in 52; the test passed regardless. 5 seeded controls.
- **C12.F3 adopted** (record: evidence/phase-f/configure-bounds-amendment/): INV-PROTO-1's last sentence. `configure_bounds` occurs only as `(0, 0)` with an initial configure (Smithay, W-SM-17), which xdg-shell defines as equivalent to no event. Non-zero bounds are not authorized. T1 `t1_protocol_bounds`.
- **C12.F4 adopted** (record: evidence/phase-f/epoch-time-authority/): INV-SCALE-3 gains the clock clause; M7 names the session's monotonic clock. Sealed text only: the scale policy is INC-6.
- **OB-13 split** (record: evidence/phase-f/ob-13-split/): OB-13 keeps enforcement and the query through the renderer capability interface (INC-6, T1); new **OB-52** carries the real renderer's limit on a GPU (INC-13, T2). Both OPEN. The checker treats T2 as a deferred destination.
- **Registry:** 57 invariants (8 layers), 21 formulas, 52 obligations, W-SM-1…17; docs self-test 28 fixtures.
- **Declaration:** INC-6 is NOT ratified and INC-6 exit is NOT authorized; INC-7 is experimental; the runtime remains EXPERIMENTAL pending INC-13; OB-13, OB-42, OB-44 and OB-52 are OPEN.

## PHASE F — H-1 AMENDMENT C12.F2 ADOPTED (record: evidence/phase-f/h1-amendment/)
- **Authority:** owner approval 2026-10-13 of "the corrected pre-client-rounding device-bound formulation".
- **Registry:** new layer FOUNDATION_ARCHITECTURE §C12.F2 (one row, INV-SCALE-2; formula record C12.F2.2); F-SCALE-2: `H_dev(E) = ⌊120 · D_max / s_max(E)⌋ / 120`, `+∞` at zero extent, `1/120` when the quotient is 0. F-SCALE-3 unchanged. 57 invariants, 21 formulas.
- **Checker:** 3 seeded fixtures added (25 in the self-test); the `Amended` field must name each amending layer exactly.
- **Implementation:** not on main (the scale policy is INC-6). The Core change and its proof suite are on the INC-6 experimental line.
- **Obligations:** OB-13 history updated; **OB-13 stays OPEN**. OB-42 and OB-44 unchanged.
- **Declaration:** INC-6 is NOT ratified and INC-6 exit is NOT authorized; the runtime remains EXPERIMENTAL pending INC-13.

## PHASE F — PLACEMENT AMENDMENT C12.F1 ADOPTED (record: evidence/phase-f/placement-amendment/)
- **Authority:** owner approval 2026-10-13: "Default placement is resolved once at first map, when authoritative initial window geometry becomes available, rather than at the initial surface commit."
- **Registry:** new layer FOUNDATION_ARCHITECTURE §C12.F1 (one row, INV-STAGE-1); new formula F-PLACE-1; STATE_MACHINES M1 (two rows); PROTOTYPE_DECISION_LEDGER PL-21. The sealed C12 table is unaltered; 57 invariants, 21 formulas.
- **Checker:** `tools/check-foundation-docs` reads ordered Phase F amendment layers (C12.F1, C12.F2); 4 seeded fixtures added (22 in the self-test).
- **Core:** `PlacementPolicy::CentredInView` is the default; the backends no longer select it. `OriginAtCameraCentre` stays as an explicit policy with its own tests.
- **Tests:** 41 expectations that encoded the legacy anchor re-derived from F-PLACE-1 (table in the record); 2 explicit legacy-policy tests; 3 seeded controls.
- **Not claimed:** restored or explicit placement (no such path exists), transient windows, reserved regions, placement under zoom ≠ 1, any real-application or GPU result.
- **Declaration:** INC-6 is NOT ratified and INC-6 exit is NOT authorized; the runtime remains EXPERIMENTAL pending INC-13; OB-42 and OB-44 remain OPEN.

## PHASE F — UNSAFE-INVENTORY ENFORCEMENT INTEGRATION (record: evidence/phase-f/unsafe-inventory-gate/)
- **Integrated commit:** `804ab6554860b7884eb72c3020a6136651fcf482` (fast-forward of one commit on `ede1541`; not pushed). Owner authorization: 2026-10-13.
- **Scope:** `enforcement/source-rules` (rule `unsafe-policy`) and `tools/verify-architecture` (19 seeded controls). No crate under `crates/` changes; C9 and INV-ARCH-2 are unchanged.
- **Effect:** every `unsafe` site must match exactly one row of `docs/foundation/UNSAFE_INVENTORY.md` by file, enclosing item, count and class (`test` or `production`); unlisted, duplicate, ambiguous, stale, malformed or incomplete rows fail; an inventory that merely exists authorises nothing. The workspace has no `unsafe` site and no inventory at this commit.
- **Limitation:** `unsafe` produced by expanding a macro defined in another crate is invisible to the rule (it reads source tokens). Recorded in the rule's doc comment and in the integration record §4.
- **Not claimed:** authorization of any `unsafe` site; the windowless GLES test and its inventory row remain experimental.
- **Declaration:** INC-6 is NOT ratified and INC-6 exit is NOT authorized; the runtime remains EXPERIMENTAL pending INC-13; OB-42 and OB-44 remain OPEN.

## PHASE F — RUNTIME REMEDIATION INTEGRATION — EXPERIMENTAL PENDING INC-13 (record: evidence/phase-f/runtime-remediation/)
- **Integrated candidate:** `remediation/a-runtime` `e53cb3bde018fef7c33011d82e151316292c9c94`, merged as `59723f2532fe8fb04f3148cba5d2a4529c5f716f` on main `85a74cb` (not pushed).
- **Scope:** `infynitae-wayland::session` (`Session`, `FrameTimer`, one `terminate_children`), both prototype run loops reduced to a loop body, presentation timestamps on the announced clock, repaint for a vanished client, software-renderer pixel tests of the production render path. No Core change. No authority port taken by shared code; no `Backend` trait.
- **Owner policies (2026-10-13):** (1) the runtime remains experimental pending INC-13; (2) pixman is restricted to tests and development fixtures (dev-dependency of `infynitae-backend-headless` only); (3) the headless runner produces frames on demand, never continuously; (4) shared runtime code in the adapter crate is accepted provisionally.
- **Dependencies added (test builds only):** Smithay features `renderer_test`, `renderer_pixman` as dev-dependencies; `pixman`, `pixman-sys`, `paste` in `Cargo.lock`.
- **Open findings carried:** B-1 (SIGTERM/SIGINT without cleanup; blocked on the signal-handling dependency decision); no pixel test of the GLES path; nested idle, nested input and real applications unverified; duplicated `vblank_period` helper kept with its test.
- **INC-6 implication:** the INC-6 candidate now conflicts with main in both backend loops (scale-settlement deadline and tick) as well as in Core; a design step is required.
- **Not claimed:** INC-13 entry or exit; any GLES, GPU, nested-presentation or hardware result; OB-7, OB-13, OB-25 and OB-44 are unchanged.
- **Declaration:**
  - INC-0 through INC-5 are RATIFIED; the Core visibility remediation is RATIFIED.
  - **The runtime remediation is INTEGRATED, EXPERIMENTAL pending INC-13, and NOT RATIFIED.**
  - **INC-6 is NOT ratified; INC-6 exit is NOT authorized.**
  - Host KDE/KWin session untouched; no nested compositor run for this integration.

## PHASE F — CORE VISIBILITY REMEDIATION INTEGRATION (record: evidence/phase-f/core-visibility/)
- **Integrated commit:** `85a74cbffd3692fa9965cbe6c68465d3972c6022` (local canonical main; fast-forward of 6 commits `b5b6dc4`, `2da6b23`, `be449fe`, `24536d4`, `ec11a62`, `85a74cb`; not pushed).
- **Baseline commit:** `db254c24bfbb6c5598fdb5c7bf4fd79ba12f4524` (ratified INC-5).
- **Scope:** Core only (`infynitae-core`, `tools/verify-test-efficacy`). `Stage` lookup by binary search; visibility pass scoped to the element or camera a batch changed (full pass kept for output connect, reconfigure, disconnect); per-element bounding-box cull; `frame_tick` from the membership table. No effect added, dropped or reordered; no authority moved; no dependency change. Not a spatial index (C3.8 threshold remains unevaluated; INV-RENDER-4 unaffected).
- **Origin:** viability review finding F1 (O(E²) event cost in the INC-0…5 Core), reproduced 2026-10-11.
- **Tests:** T0 differential tests against the unscoped, unculled reference (`scoped_visibility_equals_the_full_reference`, `output_changes_under_a_pan_equal_the_full_reference`), `cull_agrees_with_the_reference_at_the_view_edges`, `lookup_agrees_with_a_linear_scan_under_churn`; 16 seeded controls added.
- **Derived verification evidence (at `85a74cb`, before and after integration):**
  - Workspace test suite: 257 tests pass across 25 suites, 0 failed, 0 ignored.
  - Cargo fmt and clippy (`--workspace --all-targets --locked -- -D warnings`): PASS.
  - `./tools/verify-architecture`: PASS.
  - `./tools/verify-test-efficacy`: PASS (126 seeded controls detected, 2 judges).
  - `./tools/check-foundation-docs` and `--self-test`: PASS.
  - `./tools/verify-development-environment`: PASS.
- **Measured (T0 model, headless, `core_bench`; not C14 baselines):** at 3,000 elements a pan event 2,624 µs → 23.5 µs, a commit 2,624 µs → 0.34 µs, render-list construction 2,641 µs → 101 µs. No frame-time, presentation or input-latency claim; no renderer, GPU or hardware involved.
- **Limitations:** pan, render list, remap and hotplug remain linear in mapped elements; INV-ALLOC-1 is not met (OB-42 stays OPEN); OB-44 stays OPEN; no human review recorded.
- **INC-6 implication:** the frozen INC-6 candidate `f57315c` conflicts with this main in three files; a resolution and the correction of a pre-INC-6 debug assertion exposed by it exist only on `experimental/inc-6-on-core-85a74cb` (experimental, unratified).
- **Review verdicts:**
  - Owner integration authorization: `GRANTED` (2026-10-13, Core visibility remediation only).
  - Owner acceptance of `85a74cb` as canonical local main tip: `GRANTED` (2026-10-13).
  - Owner ratification gate: `FORMAL RATIFICATION AUTHORIZED` (2026-10-13).
- **Declaration:**
  - INC-0 through INC-5 are RATIFIED.
  - **The Core visibility remediation is INTEGRATED and RATIFIED.**
  - **INC-6 is NOT ratified;** the runtime remediation is NOT integrated.
  - Host KDE/KWin session untouched; no nested compositor run.

## PHASE F — INC-5 FORMAL RATIFICATION AND SEAL
- **Ratified implementation commit:** `4aaf5a3fc770f611beefbc24ee9469aada27e0dd` (canonical main).
- **Ratified baseline:** Phase F INC-5 candidate `4d2fb3dc01ece2fabd37f1467c9f9831795e7ed4` and tiny-compositor candidate `4e7a7f892491df198216c384fae0a0ef74e5472a`.
- **Ratification scope:** Phase F pointer-constraint architecture and protocol behavior (`zwp_pointer_constraints_v1`, `zwp_relative_pointer_v1`, M13, M14, F-LOCK-1, F-CONFINE-1, F-RELOCATE-1, batch-end reconciliation with DEFER strategy under interactions and grabs, zero-output transitions, and popup preemption restoration).
- **Evidence locations:** `docs/foundation/evidence/phase-f/INC-5/`, `crates/infynitae-backend-headless/tests/t1_pointer_constraints.rs`, `crates/infynitae-core/src/pointer.rs`.
- **Review verdicts:**
  - Independent adversarial review: `PASS — RATIFICATION RECOMMENDED` (Level A = 0, Level B = 0).
  - Owner ratification gate: `FORMAL RATIFICATION AUTHORIZED`.
- **Final defect counts:** Level A = 0, Level B = 0.
- **Level-C residual observations (non-blocking for INC-5 ratification):**
  - **C-1 (Vendor Upstream Compiler Warnings):** 5 cosmetic compiler warnings originate in pinned `vendor/smithay-0.7.0` (unused tracing imports, deprecated `Atomic::fetch_update`, integer function pointer cast in shm pool); zero warnings in Infynitae workspace crates.
  - **C-2 (OB-20 Physical Game Execution Allocation):** Physical game execution on bare-metal DRM/KMS outputs remains formally allocated to INC-17 (`T5 PHYSICAL`). Protocol-tier obligations (TST-05 x8 C3 pan with lock, C4 pan with confinement) are verified.
  - **C-3 (OB-11 Upstream Landing Tracking):** Upstream Smithay issue submission remains an outward tracking item; Option B (patch P1 under `D-B12-PIN`) is adopted, pinned, and verified locally.
- **Obligation dispositions (`docs/foundation/FOUNDATION_TEST_PLAN.md`):**
  - **OB-31:** CLOSED. Persistent and oneshot `xlock_grab` DEFER strategy, confinement clamping (`F-CONFINE-1`), and zero-leak verified by T1 production tests (`ob31_xlock_grab_persistent_defer`, `ob31_xlock_grab_oneshot_defer`) and core unit tests.
  - **OB-3:** CLOSED. Output removal mid-interaction (pan and client grab) verified by T1 production tests (`removing_the_pan_output_orphans_the_pan`, `ob31_xlock_grab_persistent_defer`).
  - **OB-47:** CLOSED. Constraint-lifecycle regression (EE-2 E1–E11) and zero duplicate `unlocked` events under P1 verified by T1 production tests (`ob47_e1_persistent_explicit_deactivate_then_exit`, `ob47_e3_oneshot_explicit_deactivate_defunct`, `round5_pointer_constraints_stress_200_cycles`).
  - **OB-50:** CLOSED. Zero-output constraint lifecycle (EE-3) verified across persistent lock, oneshot lock, confinement, and held button by T1 production tests (`ob50_zero_output_*`).
  - **OB-51:** CLOSED. W-SM-16 popup preemption under lock verified by T1 production test (`ob51_popup_preemption_under_lock`).
  - **OB-11:** SATISFIED / CLOSED for the INC-5 decision gate under Option B (patch P1 under `D-B12-PIN`). Remains OPEN in register as destination is UP.
  - **OB-20:** Protocol part SATISFIED (`ob20_x8_c3_pan_with_lock`, `ob20_x8_c4_pan_with_confinement`). Physical hardware validation remains assigned to INC-17 (`T5 PHYSICAL`).
- **Derived verification evidence:**
  - Workspace test suite: 253 tests pass across 25 suites (core 76, T1 headless 168, backend-winit 9, etc.), 0 failed, 0 ignored.
  - Cargo fmt and clippy (`--workspace --all-targets --locked -- -D warnings`): PASS.
  - `./tools/verify-architecture`: PASS (including 6 patch P1 integrity controls).
  - `./tools/verify-test-efficacy`: PASS (all seeded mutants detected).
  - `./tools/check-foundation-docs` and `--self-test`: PASS.
  - `./tools/verify-development-environment`: PASS.
- **Ratification declaration:**
  - **Phase F INC-5 is RATIFIED.**
  - **INC-0 through INC-5 are RATIFIED.**
  - **INC-6 has NOT begun.**
  - Physical DRM/KMS hardware validation excluded; host KDE/KWin session untouched.

## PHASE F — INC-5 IMPLEMENTATION (evidence: evidence/phase-f/INC-5/)
- **Baseline commit:** `2680b99eb6d5353473ff362f5b4a904372f1fdfb` (ratified INC-4).
- **Scope & Deliverables:**
  - Protocol seam: `zwp_pointer_constraints_v1` support in `infynitae-wayland` (`LockedPointer` and `ConfinedPointer` global and surface handlers).
  - Core authority: `Core::pointer_constraint_request`, `Core::pointer_constraint_destroy`, `OBJ-21`, `M13` state machine, `F-LOCK-1` region/lifetime evaluation, and `F-CONFINE-1` continuous confinement bounding clamp.
  - Interaction preemption & reconciliation: grab preemption restoration on popup close (`Core::pointer_preemption_restored`), and output removal reconciliation (`F-RELOCATE-1`).
  - Integration test suite: 13 tests in `crates/infynitae-backend-headless/tests/t1_pointer_constraints.rs` covering OB-31 (pointer lock), OB-20 (confinement), OB-50 (confinement clamp), OB-51 (output removal under lock), OB-47 (explicit deactivate then exit under P1), and Checkpoint L 200-cycle stress test.
- **OB-11 Owner Adjudication & Patch P1 Adoption (D-B12-PIN):**
  - Owner decision: Option B — Adopt Patch P1 (minimal 4-line guard `if constraint.is_active() { constraint.deactivate(); }` in Smithay `PointerHandle::leave()`).
  - Stock defect reproduction: documented in `docs/foundation/evidence/phase-f/INC-5/OB-11-STOCK-DEFECT-REPRODUCTION.md`.
  - Canonical patch file: `patches/0001-seat-leave-deactivate-only-active-constraint.patch`.
  - Vendored dependency: pinned `vendor/smithay-0.7.0` under root `[patch.crates-io]`.
  - Strict architectural pinning: `tools/verify-architecture` enforces SHA-256 of `pointer.rs` (`9c120d4101d7b2d3907d018a400dcd52e32e3be6b6cf6b64c80d0b421bbcb512`), verifies byte-identity with crates.io registry for all other files, and enforces 6 seeded negative controls (patch removal, arbitrary patches, unpinned sources, revision changes, local modifications, dependency upgrades).
  - Wire verification: `ob47_e1` asserts `duplicate_unlocked == 0`.
  - Upstream status: tracked as outward submission; no upstream fix claimed landed. Patch remains pinned and removable when upstream lands an equivalent fix.
- **Obligation dispositions:**
  - **OB-11:** SATISFIED / CLOSED for the INC-5 decision gate under Option B (patch P1 under D-B12-PIN).
  - **OB-3, OB-20, OB-31, OB-47, OB-50, OB-51:** Satisfied and verified on the production path.
- **Declaration:**
  - INC-0 through INC-4 are RATIFIED.
  - **INC-5 has NOT been ratified.**
  - **INC-6 has NOT begun.**
  - Headless execution only; live compositor and physical display untouched.

## PHASE F — EXPERIMENTAL RUNNABLE PROTOTYPE (TINY COMPOSITOR) INTEGRATION
- **Integrated Candidate:** `4e7a7f892491df198216c384fae0a0ef74e5472a`
- **Baseline Commit:** `4d2fb3dc01ece2fabd37f1467c9f9831795e7ed4` (Phase F INC-5 candidate).
- **Scope & Deliverables:**
  - Runnable Nested Compositor: `infynitae-backend-winit::run_nested` with Winit event loop, GL presentation, and Calloop dispatch.
  - Headless Compositor Runner: `infynitae-backend-headless::run_headless` with virtual outputs and automated duration/frame bounds.
  - CLI Composition Root: `crates/infynitae/src/main.rs` with socket selection, client execution (`--exec`), sizing, and refresh rate pacing.
  - Core Initial Focus Activation: `Core::grant_initial_focus` and `FocusModel::grant_initial_focus` enforcing `INV-INPUT-8` (activation grant on first map without focus-stealing on subsequent mappings).
  - Child Process Supervision: `terminate_children` implementing bounded 500 ms graceful SIGTERM polling, SIGKILL escalation, and waitpid zombie reaping.
  - Performance & Event Hardening: Calloop generic display event loop integration, configurable refresh pacing (60–240 Hz), and suppression of redundant idle GPU buffer presentation.
  - Extended Test Suite: 5 end-to-end integration tests in `tests/t1_tiny_compositor.rs` and robust in-flight leave tracking under C Wayland backend (`client_system`).
- **Deferred Protocol Capabilities (Documented Future Increment Ownership):**
  - `wp_fractional_scale_v1`: Canonical future increment INC-6 (F-SCALE-1/2/3).
  - `wp_viewporter`: Canonical future increments INC-6 / INC-15.
  - `wp_cursor_shape_v1`: Canonical future increment INC-15.
- **Declaration:**
  - INC-0 through INC-4 are RATIFIED.
  - **INC-5 has NOT been ratified.**
  - **INC-6 has NOT begun.**
  - Live nested compositor execution enabled for testing; host KWin desktop preserved.


## PHASE F — INC-4 FORMAL RATIFICATION AND SEAL
- **Ratified implementation commit:** `4c01cca2d43e38f4f3a80e9202ec1b7ab0430179` (origin/main).
- **Ratified baseline:** INC-3 commit `1ee520fa0079d45193f2a7274b31018e9276b0ed`.
- **Review verdicts:**
  - Fifth independent adversarial review (`audit/phase-f-inc4-rereview-4/`, commit `b22da28`): `PASS — READY FOR FORMAL RATIFICATION` (Level A = 0, Level B = 0).
  - Final focused ratification audit: `PASS — FORMAL RATIFICATION AUTHORIZED` (Level A = 0, Level B = 0).
- **Final defect counts:** Level A = 0, Level B = 0.
- **Level-C residual observations (non-blocking for INC-4 ratification):**
  - **C-1:** Popup first commit already carrying a buffer is currently accepted (toplevel equivalent is rejected correctly). W-SM-6 popup portion (`ensure_configured`) remains unresolved and is deferred to INC-7.
  - **C-2:** Focused-window unmap may produce a deactivation configure; non-blocking observation.
  - **C-3:** Fifth-review obligation recommendations for OB-18 and OB-21 were corrected by the final ratification audit using the canonical obligation register (`FOUNDATION_TEST_PLAN.md`), keeping parent obligations with deferred or unfulfilled components OPEN while recording partial progress in history.
  - **Carried observations:**
    - Trusted `InputPort` holder boundary: the port is the root of trust; Core cannot inspect XKB keymaps (C4.4).
    - Pinned Smithay 0.7.0 middle-popup behavior: Smithay grab teardown unrolls cleanly to the live ancestor or active element root.
    - Two semantically equivalent Round-4 mutations (A1-a: chain end keeps grab; A1-d: reconcile to `current_grab()`) retired and documented in Round-4 matrix.
- **Obligation dispositions (`docs/foundation/FOUNDATION_TEST_PLAN.md`):**
  - **OB-4:** CLOSED. Popup-grab dismissal on interaction acquisition (CR-4, X-11b) verified by T1 production tests (`pf_ob4_interaction_acquisition_dismisses_popup_grab`, `r10_pan_with_popup_no_synthetic_events_and_clean_return`, and `a1_camerapan_overlapping_destruction`).
  - **OB-15:** CLOSED. Compositor-side repeat (K9 unused) and output removal (K8 no-op) verified by T1 production tests (`k8_ob15_output_removal_under_held_keys`, `k9_ob15_key_repeat_probe`, `t04_duplicate_presses`, `release_follows_press_owner_after_modifier_change`, and `sync_seat_key`).
  - **OB-30:** CLOSED. Port `xpopup_focus` (ER-1) to T1 against production adapter (ActiveElementFocus vs ProtocolSurfaceRoute, ordered preemption INV-INPUT-10, nested-grab W-SM-14 workaround) verified by T1 production tests (`pf_1` through `pf_7`, `ordered_preemption_exact_keyboard_wire`) and permanently gated by `./tools/verify-architecture` [5d] against negative controls NEG-1, NEG-2, and OBS-1 under probe flags.
  - **OB-18:** OPEN. The W-SM-7 popup grab panic precondition is fully satisfied at INC-4 via `grab_popup` validation, `reconcile_popup_chain`, and depth 1..4 adversarial tests (`a1_inner_popup_closes_then_key`, `a1_client_closes_whole_chain_then_key`, `a1_middle_popup_disappears_then_key`, `a1_outer_popup_disappears_then_key`, `a1_root_unmaps_then_key`, `a1_destruction_at_depth_4`). Per C-1, the W-SM-6 popup portion (`ensure_configured` / initial commit carrying buffer) remains open pending INC-7. OB-18 therefore remains OPEN in the register, blocking INC-7 exit.
  - **OB-21:** OPEN. libinput device semantics under pan accumulation (DV-4) destination is T4 (physical hardware testing); ownership remains INC-16 / T4.
- **Derived verification evidence (`ulimit -n 1024`, `CARGO_BUILD_JOBS=4`):**
  - Workspace test suite: 214 normal tests pass (core 70, T1 144: adversarial 13, input 17, keyboard 36, outputs 12, parity 6, popup_focus 32, popup_lifetime 17, slice 4, transform 7); probe-feature `t1_popup_focus` 36 tests pass (218 total).
  - Cargo fmt and clippy (`--workspace --all-targets --locked -- -D warnings`): PASS.
  - `./tools/verify-architecture`: PASS (14 source rules, 41 compile probes + 1 positive control, runtime checks [5b] 4, [5c] 5, [5d] 4, 55 seeded + 3 judge controls).
  - `./tools/verify-test-efficacy`: PASS (110 seeded, 110 DETECTED, 0 MISSED, 0 BROKEN, + 2 judge).
  - `./tools/check-foundation-docs` and `--self-test`: PASS (57 invariants, 20 formulas, 51 obligations).
  - `./experiments/c1-authority-spike/verify.sh`: PASS.
  - `./tools/verify-development-environment`: PASS.
  - CI verification on implementation commit `4c01cca2d43e38f4f3a80e9202ec1b7ab0430179`: run `37476629305` (all jobs green: foundation-docs, verify-architecture, workspace-build).
- **Ratification declaration:**
  **Phase F INC-4 is RATIFIED.**
  No production behavior was changed during ratification.
  The live system was untouched.
  **INC-5 has NOT begun.**

## PHASE F — INC-4 CORRECTIVE PROGRAM — ROUND 4 (record: evidence/phase-f/INC-4/corrective-r4/)
- **Review:** `INC-4 FAIL — FURTHER CORRECTION REQUIRED` (fourth review, `audit/phase-f-inc4-rereview-3/`, ingested byte-for-byte as `dc4d52e`; reviewed HEAD `60e5429`). Findings: Level A 1 (A1), Level B 3 (B1–B3), Level C 4 (C1–C4).
- **Corrective commits:** `b320cbe` (matrix, reconstruction), `a63b7c5` (A1, B1), `9da07d7` (C3), `d75746b` (tests, oracles), `cf743e5` (verification); each passes `cargo test --workspace --locked` and `fmt` on its own.
- **A1 (Level A):** client-driven close of a popup chain of depth ≥ 2 left the keyboard grab installed and Smithay focus on a dead popup. Popup death now runs `reconcile_popup_chain`: the keyboard moves to the topmost live grabbing popup at once; when none is live, exactly once, the keyboard and pointer grabs are released by their install serials and the keyboard is routed to Core's ActiveElementFocus root. **Provenance:** introduced by INC-4 itself (`d5b7b95` reproduces it); cannot exist on the ratified INC-3 baseline `1ee520fa` (no keyboard, no popup grabs) — not an inherited foundation defect.
- **B1 (Level B):** the Round-3 R2-01 fix ran the parent's M1 lifecycle on popup death (unmapped parent re-armed, re-show unanswered). New `Core::toplevel_tree_changed` (snapshot only, never M1) serves popup death and child-surface commits; M1 runs only on the root surface's own commit. The same split removes the pre-existing child-commit variant (fourth-review O-1). R2-01 stays fixed (former popup area hits the parent).
- **B2 (Level B):** efficacy controls I and K were BROKEN (stale anchors after a rustfmt reflow); re-derived from current source. `verify-test-efficacy` and `verify-development-environment` PASS.
- **B3 (Level B):** F15/F16 detected by a literal FIFO/fallback provenance test; N09d (W-SM-15) detected by the default-suite exact ORDERED keyboard-wire test; NEG-1, NEG-2, OBS-1 now run unmutated in `verify-architecture` [5d] (CI).
- **C1 correction (appended; the Round-3 text is unchanged):** the Round-3 R2-10 entry called the provenance fallback "Rule 2 (output containing the pointer cursor)". That was wrong. Canonical D-B6-BOUNDARY:
  - **Rule 1** is the output of the input event that caused the popup — the grab serial, *else the seat's last input* — if the parent is visible there.
  - **Rule 2** is the parent's frame-clock output (highest-refresh observing output).
  - **Rule 3** is no constraint adjustment.

  The implementation already matches. A recorded serial resolves to its causing output; a missing, evicted or keyboard serial uses the pointer's current output under Rule 1's "else the seat's last input" clause; Rule 2 applies when the parent is not visible on that output. No code change.
- **C2 correction (appended):** the Round-3 entry claimed
  - "34 compile probes", "54 seeded controls" and `verify-test-efficacy` PASS;
  - that `sync_seat_key` "asserts".

  The fourth review measured 40 + 1 positive compile probes, 55 seeded + 3 judge architecture controls, and efficacy **FAIL (2)** with 99 seeded (97 detected, 2 BROKEN) + 2 judge. `sync_seat_key` fails closed without an assert. Cause: the record was written without mechanically deriving the counts or running the final gate.
- **C3 (disposition):** the token now carries the key event it authorizes (`verify_input_port(port, key, pressed)`, `sync_seat_key(token)`). The duplicate/orphan transition gate moved from the headless backend into `sync_seat_key` (one implementation for every backend). Compile probe p31 pins single use (E0382). Residual (owner-level): a trusted port holder can mint a proof without submitting the event; the port is the root of trust, and Core cannot hold XKB (C4.4).
- **C4 (disposition):** KV-2 reference model tracks CameraPan from the test sequence; F-4 asserts exactly one enter since the focus change; the provenance test checks FIFO order, retention and fallback, not only size; NEG/OBS gated.
- **Equivalent mutations retired/recorded:** "chain end keeps the keyboard grab" and "reconcile to the chain's `current_grab()`" — argued from pinned Smithay and the grab-record invariant in the matrix §Closure.
- **Verification (mechanically derived; `ulimit -n 1024`, `CARGO_BUILD_JOBS=4`):**
  - **Cargo:** check, build and test PASS. **214 tests** = core 70 + T1 144 (adversarial 13, input 17, keyboard 36, outputs 12, parity 6, popup_focus 32, popup_lifetime 17, slice 4, transform 7); probe-feature `t1_popup_focus` 36 (218 in total). fmt and clippy `-D warnings` PASS.
  - **`verify-architecture`:** PASS. 14 source rules; 41 compile probes + 1 positive control; runtime checks [5b] 4, [5c] 5, [5d] 4; 55 seeded + 3 judge controls.
  - **`verify-test-efficacy`:** PASS. 110 seeded, 110 DETECTED, 0 MISSED, 0 BROKEN, + 2 judge.
  - **Other gates:** foundation checker PASS (57 invariants, 20 formulas, 51 obligations); self-test PASS; authority spike PASS; `verify-development-environment` PASS.
  - **Fresh mutation round:** 22 mutations; 20 detected, 0 meaningful missed, 2 equivalent, 0 broken.
  - **Stress:** 200 cycles; asserted clean end state; FDs 10 → 10; 0 violations.
- **INC-3 regression:** all ratified INC-3 suites and efficacy controls pass, including CameraPan/KeepFocus/true-position, zero output, ProjectionFrame invalidation, transform/hit agreement and the bounded-resource test.
- **Obligations:**
  - OB-4: OPEN, behaviour met; awaiting review.
  - OB-15: OPEN, behaviour met; awaiting review.
  - OB-30: OPEN, behaviour and NEG/OBS gate met; awaiting review.
  - OB-18 (popup part): closable only at ratification.
  - OB-21: OPEN (INC-16 / T4).
- **CI (exact HEAD):** run 37475576796 on `80a5328a6ba338ae4d2a8025e35b471c83649628` — foundation-docs, verify-architecture (incl. [5d]) and workspace-build all success; 214 tests; `verify-test-efficacy: PASS` (110 DETECTED, 0 FAIL). This entry's own commit is verified by the CI run on it.
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** STOP for the fifth independent adversarial review. INC-4 is NOT ratified. INC-5 has NOT begun.

## PHASE F — INC-4 CORRECTIVE PROGRAM — ROUND 3 (record: evidence/phase-f/INC-4/corrective-r3/)
- **Review:** `INC-4 FAIL — FURTHER CORRECTION REQUIRED` (dossier preserved byte-identically in `audit/phase-f-inc4-rereview-2/`, commit `033be36`). Findings: Level A 0, Level B 7 (R2-01–R2-07), Level C 3 (R2-08–R2-10).
- **Corrective Finding Matrix:** `evidence/phase-f/INC-4/corrective-r3/CORRECTIVE_FINDING_MATRIX.md`.
- **Core Architecture & Authority Corrections:**
  - **R2-01 (Level B, pre-existing INC-3 foundation defect resolved):** In `crates/infynitae-wayland/src/handlers.rs`, implemented `XdgShellHandler::popup_destroyed` callback. When client destroys a popup surface, Wayland handler purges the popup from popup state, clears any active popup pointer grab if the destroyed surface held it, and notifies the core via `element_committed` to re-extract the parent toplevel snapshot immediately without requiring an asynchronous parent redraw commit. Note: This defect pre-existed in the ratified INC-3 foundation baseline (`1ee520fa`), where destroyed popup surfaces left stale snapshots and grabbed states; it is now fully resolved and guarded by permanent regressions `t99g_popup_destroyed_by_client_then_clicks` and `t99j_stale_popup_area`.
  - **R2-02 (Level B):** Bound `InputAuthToken` capability proof. `InputAuthToken` is non-Send (`_not_send: NotSend`) and bound to the issuing `Core` instance via `core_id: u64`. `WaylandState::sync_seat_key` consumes the token by value (`token: InputAuthToken`), asserting `token.core_id() == self.core.id()`. Token cannot be forged, transferred across threads, or reused. Probes `p25_call_sync_seat_key_without_auth_token` and `p30_input_auth_token_send` enforce capability invariants at compile time. Runtime tests `t02b_token_is_bound_to_its_core` and `t02c_token_consumed_and_cannot_be_reused` verify one-shot consumption and core binding.
  - **R2-03 (Level B):** Adapter keyboard encapsulation. Restricted `keyboard_handle(&self)` visibility to `pub(crate)`. Replaced direct keyboard handle access in backend tests with read-only state observers (`keyboard_is_grabbed`, `keyboard_current_focus`, `keyboard_modifier_state`, `serial_outputs_len`). Added enforcement probe `p29_keyboard_handle_outside_adapter` (verifying compile error E0624 outside `infynitae-wayland`) and structural source rule `smithay-keyboard-only-in-adapter` in `tools/verify-architecture`.
  - **R2-04 (Level B):** Pre-Smithay duplicate key/modifier filter. In `crates/infynitae-backend-headless/src/lib.rs`, `keyboard_key` checks `is_transition`: physical duplicate presses (already pressed) and orphan releases (already released) bypass `sync_seat_key`, preventing seat XKB modifier corruption. Tested independently for regular keys, Shift, Super, Ctrl, and Alt across repeat press, multi-repeat, focus transfer, zero outputs, and release sequences in `t04_duplicate_presses`.
  - **R2-05 (Level B):** Pan dismissed popup grab & synthetic motion boundary. Implemented `PanDismissedPopupGrab` and `set_pan_dismissed_pointer_grab` in `crates/infynitae-wayland/src/pointer.rs`. At CameraPan acquisition (`Effect::DismissPopupGrab`), replaces `PopupPointerGrab` with `PanDismissedPopupGrab` keeping focus (`Focus::Keep`) and leaking zero events (preserving INV-INPUT-3). Upon pan completion/reconciliation, unsets with `restore_focus: false` and delivers the true-position motion without synthetic motion replay. In `outputs.rs`, `Effect::KeyboardFocus` checks `popup_pointer_grab_serial` to cleanly unset 3-level nested grabs. In `pointer.rs`, `PointerZeroOutputEnd` verifies the current grab target is an alive popup surface before reinstalling. Permanent regression tests `t01b_nested_pan_dismissal`, `t06a_three_level_nested_distinct_serials_focus_transfer`, `t06d_implicit_button_grab_then_zero_outputs_with_popup`.
  - **R2-06 (Level B):** Unconfigured key bindings. Removed test fixtures `KEY_B`, `KEY_ESC`, `KEY_LEFTSHIFT` from `CANONICAL_BINDINGS` in `crates/infynitae-core/src/input/keys.rs`. Only `Super+Enter -> FocusNext` remains canonically bound. Unconfigured keys (Super+B, idle Esc) are never swallowed and pass through to focused client wire. Permanent regression test `t11_unconfigured_keys_are_never_swallowed`.
  - **R2-07 (Level B):** Verification gaps and mutant detection. Added M08 test `serial_outputs_storage_bounded_at_capacity` in `tests/t1_outputs.rs`. In `r17_long_lived_stress_bounded_state`, asserted zero protocol violations (`violations.len() == 0`) and zero depressed modifiers (`depressed == 0`). Updated N08 and N09 mutants in `tools/verify-test-efficacy` targeting default-suite tests `t01b_nested_pan_dismissal` and `t06a_three_level_nested_distinct_serials_focus_transfer`, and added M08 mutant control.
  - **R2-08 (Level C):** Removed production ordering instrumentation (`popup_preemption_pending`, `popup_preemption_ordering_violated`) and production `assert!` from `crates/infynitae-wayland`. Preemption ordering verified via external observers and test assertions.
  - **R2-09 (Level C adjudication / canonicalization):** Formally canonicalized owner adjudication for INC4R-10: Smithay synthetic focus restoration motion is strictly an internal Smithay mechanism and must NEVER cross the client-visible protocol wire in states where an Infynitae invariant (such as INV-INPUT-3 during camera pan) requires suppression. The camera pan grab isolation (`PanDismissedPopupGrab`) cleanly preserves this boundary.
  - **R2-10 (Level C adjudication / canonicalization):** Formally canonicalized D-B6-BOUNDARY Rule-1 fallback: when an interaction serial is unrecorded, evicted from the serial LRU ring buffer (>64 entries), or caused by a non-pointer event (e.g. keyboard serial without a tracked output), `Core::pointer_output()` correctly falls back to Rule 2 (output containing the pointer cursor, or primary output if unmapped), maintaining deterministic output assignment without panic or stall.
- **Enforcement & Verification:**
  - `verify-architecture`: PASS (34 compile probes including p29 and p30, 54 seeded controls, all structural source rules passing).
  - `verify-test-efficacy`: PASS (all mutants detected including M08, N08, N09; judge controls PASS).
  - `check-foundation-docs`: PASS.
  - `verify-development-environment`: PASS.
  - Full workspace regression suite: PASS (195 tests, 100% passing under `ulimit -n 1024`).
- **NEXT:** STOP for fourth independent adversarial review. Do NOT declare ratified. Do NOT begin INC-5.

## PHASE F — INC-4 CORRECTIVE PROGRAM — ROUND 2 (record: evidence/phase-f/INC-4/corrective-r2/)
- **Review:** `INC-4 FAIL — FURTHER ARCHITECTURAL CORRECTION REQUIRED` (dossier preserved byte-identically in `audit/phase-f-inc4-rereview/`, commit `9efcf85`). Findings: Level A 1 (INC4R-01), Level B 8 (INC4R-02–INC4R-09), Level C 3 (INC4R-10–INC4R-12).
- **Corrective Finding Matrix:** `evidence/phase-f/INC-4/corrective-r2/CORRECTIVE_FINDING_MATRIX.md` (commit `99ec2f0`).
- **Core Architecture & Authority Corrections:**
  - **INC4R-01 (Level A blocker):** In `Effect::DismissPopupGrab(target)`, resolved `target_surface` and invoked `kb.set_focus` after `g.ungrab(PopupUngrabStrategy::All)` and `keyboard_unset_grab()`. Eliminates focus divergence and protocol violations where keys reached surfaces without entered keyboard focus. Permanent regression tests `r16_keyboard_focus_after_pan_dismissal` and `r17_long_lived_stress_bounded_state`.
  - **INC4R-02 (Level B):** Capability sealing. Minted non-forgeable `InputAuthToken` via `Core::verify_input_port(&self, &InputPort)`. `sync_seat_key(&InputAuthToken)` requires proof of authorization. Compile probe `p25_call_sync_seat_key_without_auth_token` ensures capability cannot be forged. Runtime test `set_focus_rejects_foreign_port` verifies refusal on foreign ports.
  - **INC4R-03 (Level B):** Adapter surface minimality. Removed unconstrained `WaylandState::test_set_keyboard_grab`, `test_simulate_focus_without_core`, `set_ordered_preemption_strategy`, `set_nested_grab_keep_pointer` from default builds; gated behind explicit compile-time test probe features `probe_test_keyboard_grab`, `probe_preemption_strategy`, and `probe_nested_grab_keep_pointer`. Enforcement compile probes `p26`, `p27`, `p28` ensure production builds lack these test escape hatches.
  - **INC4R-04 (Level B):** Wire repeat semantics. In `entry.rs`, repeat client press is ignored (`owner(key) == Some(Owner::Client)` returns `Vec::new()`), matching Linux input and Wayland protocol expectations and preventing spurious repeat forwarding.
  - **INC4R-05 (Level B):** Anchor output causality. `place_popup` threads causing event serial `Option<Serial>` and looks up `serial_output(s)` (D-B6 Rule 1). Tested in `r15_rule1_uses_the_causing_event_output`. Mutant N21 permanently killed.
  - **INC4R-06 (Level B):** Popup preemption lifecycle across zero outputs and distinct serials. Updated `outputs.rs` to check both `g.serial()` and `g.previous_serial()` on preemption. Updated `pointer.rs` to reinstall popup grab across zero-output reconnect. Tested in `r09b_nested_preemption_with_distinct_serials` and `r12_popup_grab_semantics_across_zero_outputs`.
  - **INC4R-07 (Level B):** Headless intake modifier authority. Backend `pointer_button` uses seat XKB modifier state as sole source of truth (`state.seat_modifiers()`), not synthetic or external flags.
  - **INC4R-08 (Level B):** Verification gaps and surviving mutants resolved. Permanent tests and controls added for N08 (OB-4 keyboard unset), N09 (focus before ungrab ordering check), N17 (`set_focus` port check), N18 (`set_focus` mapped check), and N21 (rule-1 wiring). Circular KV-2 oracle replaced with independent truth oracle.
  - **INC4R-09 (Level B):** Test and control inventory mechanically exact and audited across workspace.
  - **INC4R-10 (Level C adjudication):** Formally adjudicated: Smithay synthetic focus restoration motion is acceptable ONLY as an internal implementation mechanism when it cannot cross client-visible boundary where an Infynitae invariant requires suppression. Canonical invariants (INV-INPUT-3) strictly preserved.
  - **INC4R-11 (Level C adjudication):** Formally adjudicated: Canonical product key binding policy vs test fixtures. Esc binding only active during in-progress interaction; unmapped keys pass through to client in idle state.
  - **INC4R-12 (Level C adjudication):** Formally adjudicated: Append-only integrity of `CHECKPOINT.md` restored. Historical pre-INC-4 entries restored byte-identically; Round 1 and Round 2 corrective entries appended.
- **Enforcement & Verification:**
  - `verify-architecture`: PASS (32 compile probes, 54 seeded controls, all structural source rules passing).
  - `verify-test-efficacy`: PASS (73/73 seeded behavioural defects detected, including N08, N09, N17, N18, N21; judge controls PASS).
  - `check-foundation-docs`: PASS.
  - `verify-development-environment`: PASS.
  - Full workspace regression suite: PASS (100% passing under `ulimit -n 1024`).
- **NEXT:** STOP for third independent adversarial review. Do NOT declare ratified. Do NOT begin INC-5.

## PHASE F — INC-4 CORRECTIVE PROGRAM — ROUND 1 (record: evidence/phase-f/INC-4/corrective/)
- **Review:** `INC-4 FAIL — ARCHITECTURAL CORRECTION REQUIRED` (dossier preserved byte-identically in `audit/phase-f-inc4-review/`, commit `19231ef`). Findings: Level A 1 (F01), Level B 11 (F02–F09, F12, F14, F16), Level C 4 (F10, F11, F13, F17), Observation 1 (F15).
- **Corrective Finding Matrix:** `evidence/phase-f/INC-4/corrective/CORRECTIVE_FINDING_MATRIX.md` (commit `aa216d3`).
- **Core Architecture & Authority Corrections:**
  - **F01 (Level A):** Intake authority restored. Removed `Core::key_ownership_mut()`. Routed keyboard intake through `Core::physical_input(&mut InputPort, PhysicalEvent::KeyboardKey)`. Core evaluates bindings and ownership, emitting `Effect::KeyboardForward` and `Effect::KeyboardIntercept`. `ElementId::new` and ownership press/release made crate-private (`pub(crate)`). Probes `p22_mutate_key_ownership`, `p23_forge_element_id`, `p24_set_focus_without_port` added.
  - **F02 (Level B):** Removed public test switch `k3_enforced` from `WaylandState`. K3 observe mode enforced unconditionally in Core (`entry.rs`).
  - **F03 (Level B):** Fixed K1/K2 dispatch. Re-press of held compositor key consumed (`KeyboardIntercept`). Orphan release dropped (never forwarded).
  - **F04 (Level B):** Plain Escape only cancels when an interaction is actively in progress (`interaction.active()`); forwarded to focused client otherwise.
  - **F05 (Level B):** CameraPan acquisition dismisses popup grabs via `Effect::DismissPopupGrab` without invoking `ptr.unset_grab`, preventing synthetic motion replay.
  - **F06 (Level B):** D-B6-BOUNDARY Rule 1 placed on production path via `Core::pointer_output()`.
  - **F07 (Level B):** Port-gated `Core::set_focus` validates target is mapped (`Lifecycle::Mapped`). `mapped_elements()` added to `Stage`. Unmapping focused element triggers target loss.
  - **F08 (Level B):** Refused grab on unfocused/mismatched surface immediately sends `xdg_popup.popup_done` and returns.
  - **F09 (Level B):** `self.popups.cleanup()` invoked prior to grab preemption so dead grabs do not unset unrelated implicit button grabs. Grab state cleared across zero-output transitions if popup grab was active.
  - **F10 (Level C):** Formally adjudicated: Smithay synthetic motion on ungrab is standard Wayland behavior, isolated under CameraPan, tracked under W-SM-16 for INC-5 locks.
  - **F11 (Level C):** Formally adjudicated: Successor fallback without focus history transitions to `None` (`Unfocused`). Arbitrary z-order fallback eliminated.
  - **F12 (Level B):** Shipped verification rebuilt with independent oracles, non-circular assertions, permanent regression tests ATK-01..ATK-18, and RES-1 resource stability.
  - **F13 (Level C):** Formally adjudicated: Reassigned OB-21 to INC-16 / T4 physical libinput intake.
  - **F14 (Level B):** Corrected test and control counts across all evidence documents.
  - **F15 (Observation):** Reconciled brief SHA-1 hash to actual reviewed commit `d5b7b95cbc910c48377c72ae94ccec2b0c05e140`.
  - **F16 (Level B):** Unified modifier authority via `WaylandState::seat_modifiers()`. Pointer button intake merges seat XKB modifiers.
  - **F17 (Level C):** Formally adjudicated: Unmapping or destroying a parent window dismisses its child popups via `popup_done`.
- **Enforcement & Verification:**
  - `verify-architecture`: PASS (34 compile probes including p22, p23, p24; all structural source rules passing).
  - `verify-test-efficacy`: PASS (all 14 previously surviving mutations F, I, J, K, L, N, P, Q, R, T, U, V, W, AB permanently detected; judge controls PASS).
  - `check-foundation-docs`: PASS.
  - `verify-development-environment`: PASS.
  - Full workspace regression suite: PASS (100% under `ulimit -n 1024`).
- **NEXT:** STOP for independent adversarial re-review. Do NOT declare ratified. Do NOT begin INC-5.

## PHASE F — INC-4 IMPLEMENTATION (evidence: evidence/phase-f/INC-4/)
- **Scope:** PRODUCT, HEADLESS, canonical INC-4 (mapping preserved before implementation: `IMPLEMENTATION_MAPPING.md`).
  - **core:**
    - `input::keys`: canonical bindings (Super+Enter -> FocusNext, Super+B -> Noop, Esc -> Cancel, Shift -> Noop), pure binding resolver;
    - `input::focus`: `FocusModel` (`OBJ-22`, `M11`, `INV-INPUT-8`), explicit focus transitions, `focus_next`, target loss successor selection with live element fallback;
    - `input::mod`: `InputState` extended with `key_ownership: Ownership` (`OBJ-25`);
    - `entry.rs`: click-to-focus on client button press, `Effect::DismissPopupGrab` on CameraPan acquisition (`OB-4`);
    - `protocol.rs`: `Effect::KeyboardFocus`, `Effect::ToplevelConfigure`, `Effect::DismissPopupGrab`; popup constraint target calculation with anchor-output rule 1 (`D-B6-BOUNDARY`, `INC3-F08`).
  - **adapter (`infynitae-wayland`):**
    - `keyboard.rs`: `WaylandState::keyboard_key` two-phase dispatch (`input_intercept` + `input_forward`), K1–K9 ownership semantics, K3 observe mode enforcement, `PreemptionStrategy` configuration;
    - `pointer.rs`: strictly encapsulated grab helpers (`set_popup_pointer_grab`, `unset_pointer_grab`, `set_keyboard_grab`, `unset_keyboard_grab`) preserving `smithay-pointer-only-in-adapter-pointer` compliance;
    - `handlers.rs`: `XdgShellHandler::grab` with `W-SM-7` root checks and `W-SM-14` nested grab retention; `toplevel_destroyed` successor focus transition;
    - `outputs.rs`: atomic ORDERED preemption under `Effect::KeyboardFocus` (ungrab(All) -> kb.unset -> ptr.unset -> set_focus) and `Effect::ToplevelConfigure` (sets/unsets `Activated`).
  - **headless backend:** `keyboard_key` intake validating port authority via `state.core().check_port(port)`.
- **Not implemented (owned later):**
  - Pointer constraints, locks, and confinement (INC-5);
  - Epochs and frame pacing (INC-6);
  - Fullscreen and maximize (INC-7);
  - Real-client integration and kick (INC-8);
  - Zoom navigation and continuous scaling (INC-9/10).
- **Tests:** 100% passing under `ulimit -n 1024`.
  - `t1_keyboard.rs`: 13 tests (K1–K9, NEG-K3 modifier control, OB-15 output removal, OB-15 repeat probe).
  - `t1_popup_focus.rs`: 13 tests (PF-1 through PF-7, PF-OB4 pan dismissal, PF-WSM7 unmapped grab rejection, NEG-1 naive control, NEG-2 pointer-first control, OBS-1 anvil order observation).
  - Full workspace regression suite passes with 0 failures and 0 warnings.
- **Enforcement:**
  - `verify-architecture`: PASS (31 compile probes, 54 seeded controls, all structural source rules intact).
  - `verify-test-efficacy`: PASS (68/68 seeded behavioural defects detected, including 5 new INC-4 mutation controls).
  - `check-foundation-docs`: PASS (57 invariants, 20 formulas, 51 obligations intact).
  - `verify-development-environment`: PASS.
- **Obligations:**
  - None closed by implementation. All obligations remain pending independent ratification review.
- **NEXT:** STOP for independent adversarial review by another agent. Do NOT declare ratified. Do NOT begin INC-5.

## PHASE F — INC-3 RATIFICATION (record: evidence/phase-f/INC-3/corrective/)
- **Review:** `INC-3 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION` (dossier recorded unchanged in `85d128a`). Findings INC3-F09 (Level B) resolved; INC3-F08 (Level C) and INC3-F06 (Observation) adjudicated.
- **Corrective patch `0187344` (tests and controls only; production code unchanged):**
  - **INC3-F09:** Resolved file-descriptor exhaustion under standard limit `ulimit -n 1024`. Single shared dynamically-resizing shm pool in test harness `Probe` reduces shm descriptors from 192 client/server FDs to 1 client/server FD per probe client. `Drop for Lockstep` deterministically terminates server client and purges buffer resources via `dispatch_single_client`. Peak test suite FD usage reduced from >1939 to 23 FDs (14 FDs in `ee1_c5`), returning to baseline 9 FDs.
  - **Regression test:** Permanent test `inc3_f09_bounded_resource_cycles` in `crates/infynitae-backend-headless/tests/t1_transform.rs` asserts bounded FD growth ($\le 5$) over 20 repeated Lockstep + 192-subsurface cycles.
- **Tooling isolation `ffc9725` (INC3-F06):** Isolated `target/test-efficacy-<pid>-<rand>` per execution in `tools/verify-test-efficacy` with automatic cleanup, eliminating concurrency races.
- **Adjudication (INC3-F08):** Confirmed Level C finding that popup anchor-output rule 1 belongs canonically to INC-4 (`OB-4` / `OB-18`).
- **Obligations:**
  - `OB-45`: **CLOSED** (Render and hit-test agreement under coordinate transformations across all 10 cases passes cleanly under `ulimit -n 1024`; 0 mismatches; sealed control fails 9/9 non-Normal cases).
  - Still `OPEN`: `OB-50` (constraints in INC-5), `OB-3` (constraint reconciliation in INC-5), `OB-21` (T4 intake in INC-4), `OB-4` (popup grab handling in INC-4), `OB-1`, `OB-6`, `OB-11`, `OB-14`, `OB-17`, `OB-48`.
- **Verification:** everything PASS under `ulimit -n 1024`.
  - Cargo: 122 tests (66 core T0, 50 distinct T1, 6 oracle self-tests), fmt, clippy `-D warnings`.
  - Verifiers: `verify-architecture` (31 compile probes, 54 seeded controls); `verify-test-efficacy` (78/78 detected, judge controls PASS); development environment verifier PASS.
- **Findings:** open Level A 0, Level B 0, Level C 2 (INC1-F01 in INC-7, INC3-F08 in INC-4).
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** INC-4 only upon explicit owner authorization. INC-4 has NOT begun.

## PHASE F — INC-3 (implemented on baseline `6470e1a`; evidence: evidence/phase-f/INC-3/)
- **Scope:** PRODUCT, HEADLESS, canonical INC-3 (mapping written before code: `IMPLEMENTATION_MAPPING.md`).
  - **core:**
    - `input::{ownership, interaction, bindings}`;
    - `PhysicalEvent` variants (motion, button with a modifier snapshot, axis);
    - `physical_input`;
    - `navigation::CameraSet::pan` + `pan_target` (F-PAN-1, R2, saturating at ±2³⁶);
    - `pointer::{moved, locate}` (Arrangement union clamp);
    - `Core::route_pointer` through `hit_test`;
    - M10 pointer effects on connect, reconfigure and disconnect;
    - orphaning (C4.6);
    - a camera change drops the output's ProjectionFrame.
  - **adapter:**
    - seat `seat0` with a pointer, `zwp_relative_pointer_manager_v1`;
    - `pointer` module (the only `PointerHandle` caller);
    - the zero-output grab (`Focus::Clear`, then `motion(None)`);
    - Canvas↔Smithay global conversion in `convert`.
  - **headless backend:** injected virtual-mouse input; `vblank` produces the output's frame.
- **Not implemented (owned later):**
  - keyboard and XKB, popup grabs, D-B6-BOUNDARY rule 1 (INC-4);
  - constraints and reconciliation with locks (INC-5);
  - epochs (INC-6), fullscreen (INC-7), kick (INC-8);
  - zoom (gated by OB-24, INC-9/10).
- **Tests:** 121 passing.
  - T0 66 (13 new).
  - T1: 49 distinct tests (30 new: x8 C1/C6/C7/C8/C9, EE-3, orphaning, adversarial A–N, EE-1 C5 across 10 transform cases with the sealed hit rule failing 9/9, pan render/hit agreement), plus the oracle self-test in each of the 6 T1 binaries.
- **Enforcement:**
  - `verify-architecture` PASS: 31 compile probes; 54 seeded controls; new rules `effects-only-from-core` and `smithay-pointer-only-in-adapter-pointer`; [5c] INV-INPUT-3 bypass switch caught on T0 and T1; `physical_input` port binding at runtime (DEFERRED: `shell_command` only).
  - `verify-test-efficacy` PASS (76/76, 30 new INC-3 controls).
  - Checker PASS (self-test PASS); spike PASS; environment verifier PASS.
  - fmt and clippy `-D warnings` clean. No new dependencies; smithay `=0.7.0`; no unsafe; P1 not applied.
- **Obligations (none closed):**
  - OB-45: all parts' pass criteria met (tracker INC-2, hit INC-3); OPEN pending review.
  - OB-50: pointer part satisfied; OPEN (INC-5).
  - OB-3: CameraPan orphaning satisfied; OPEN (INC-5).
  - OB-21: T0 part; OPEN (T4).
  - OB-4: acquisition point only; no popup grab can exist before INC-4; OPEN.
- **Findings:** no new Level A/B/C; observations INC3-F01…F08 (README §8). Open: Level C 1 (INC1-F01, INC-7).
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** independent adversarial review of INC-3. Do not begin INC-4.

## PHASE F — INC-2 RATIFICATION (record: evidence/phase-f/INC-2/corrective/)
- **Review:** `INC-2 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION` (dossier recorded unchanged in `b229462`). Findings INC2-F07 (Level B) and INC2-F01 (Level C) adjudicated.
- **Corrective patch (tests and controls only; production code unchanged):**
  - **INC2-F07:** `reconfiguration_drops_the_output_frame` in `crates/infynitae-core/src/entry.rs` verifying that mode shrink, 90° transform change, and scale change immediately invalidate and drop the cached `ProjectionFrame`, preventing stale hit-testing or frame reuse before a new frame is rendered.
  - **Controls:** Permanent mutation control in `tools/verify-test-efficacy` (INC2-F07: neutralizing frame invalidation on reconfiguration), detected (46/46).
- **Decision E Ratification:**
  - Formally ratified Decision E: OB-49 ownership correction transferring protocol fullscreen configure to INC-7 (`INV-PRES-1`, `OB-22`/`OB-32`) and ScaleInteractionEpoch settlement to INC-6 (`ScaleInteractionEpoch`, `OB-13`/`OB-47`).
  - All INC-2 reconfiguration criteria and runtime 2i negative controls are satisfied.
  - OB-49 is marked `CLOSED`.
- **Verification:** everything PASS.
  - Cargo: 76 tests (53 core, 19 distinct T1 + 4 self-tests), fmt, clippy `-D warnings`.
  - Verifiers: `verify-architecture` (27 probes, 47 seeded controls); `verify-test-efficacy` (46/46).
  - Repository checks: checker 57/20/51 (self-test 18/18), spike, environment verifier.
- **Obligations:**
  - OB-49 `CLOSED`.
  - Still `OPEN`: OB-1, OB-6 (T4 part), OB-11 (P1 gate), OB-14, OB-17, OB-45 (hit-test portion owned by INC-3), OB-48, OB-50 (adapter/constraint portions owned by INC-3/5), OB-3 (interaction orphaning owned by INC-3).
- **Findings:** open Level A 0, Level B 0 (INC2-F07 resolved), Level C 1 (INC1-F01, INC-7).
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** INC-3 only upon explicit owner authorization. INC-3 has NOT begun.

## PHASE F — INC-2 (implemented: `0a03aab`; evidence: evidence/phase-f/INC-2/)
- **Scope:** PRODUCT, HEADLESS, canonical INC-2.
  - **core:** output manager (M4/M5, OutputKey, PersistedCameras, Arrangement), PointerState (lifecycle part), visibility pass (OBJ-13) and frame clock.
  - **adapter:** wl_output + xdg-output + presentation globals, enter/leave, damage trackers (`phys_O`), frame completion.
  - **headless backend:** hotplug, reconfiguration, disconnection, virtual vblank.
- **Not started (owned later):** zero-output seat grab and routed motion (INC-3), reconciliation (INC-5), epoch settlement (INC-6), fullscreen configure (INC-7), kick and fallback (INC-8).
- **Tests:** 75 passing (T0 core 52; T1 19 distinct tests plus the oracle self-test in each of the 4 T1 binaries). Runtime 2i passes with the 1:1 rule and fails with "1:1 violated" under the sealed rule, on T0 and T1 (`verify-architecture` [5b]). EE-1 tracker matrix on the production adapter, with the sealed control failing 7/7 rotated cases.
- **Enforcement:** `verify-architecture` PASS (27 probes, 47 seeded controls); `verify-test-efficacy` PASS (45/45, 21 new INC-2 controls); checker 57/20/51, self-test 18/18; no new dependencies; smithay `=0.7.0`; no unsafe; P1 not applied (OB-11 open).
- **Obligations:** OB-49 mechanically satisfied except items that cannot execute yet (fullscreen configure, epoch settlement); OB-45 (tracker/membership/damage part), OB-50 (core part) and OB-3 (removal part) partly satisfied; all `OPEN`.
- **Findings:** Level A 0, Level B 0, Level C 2 (INC1-F01, INC2-F01).
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT (at implementation):** independent adversarial INC-2 review. Superseded by the ratification entry above.

## PHASE F — INC-1 RATIFICATION (record: evidence/phase-f/INC-1/corrective/)
- **Review:** `INC-1 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION` (dossier recorded unchanged in `38c60ca`). Findings INC1-F06 and INC1-F07 (Level B); INC1-F01…F05 adjudicated.
- **Corrective patch `1fa72af` (tests and controls only; production code unchanged):**
  - **INC1-F06:** an independent exact-integer unprojection and region oracle for T1, plus a 56-row hand-derived T0 table.
  - **INC1-F07:** T0 and T1 coverage of unshown surfaces.
  - **Controls:** 7 permanent mutation controls (ADV-MUT-3 ×3, sign inversion, camera-offset omission, ADV-MUT-2 ×2), all detected.
- **Verification:** everything PASS.
  - Cargo: 47 tests, fmt, clippy.
  - Verifiers: `verify-architecture`; `verify-test-efficacy` 25/25.
  - Repository checks: checker 57/20/51 (self-test 18/18), spike, environment verifier.
  - CI: run 37291919770 on `1fa72af`.
- **Obligations:**
  - OB-2 and OB-26 `CLOSED`.
  - Still `OPEN`: OB-1, OB-6 (T4 part), OB-14, OB-17, OB-49 (runtime 2i, INC-2).
- **Findings:** open Level A 0, Level B 0, Level C 1 (INC1-F01, INC-7).
- **NEXT (at ratification):** INC-2 upon explicit owner authorization. Superseded by the INC-2 entry above.

## PHASE F — INC-1 (evidence: evidence/phase-f/INC-1/)
- **Scope:** PRODUCT, HEADLESS, exactly the canonical INC-1.
  - **core:** `stage`, `snapshot` + F-ORIGIN-1, `projection` (F-VIEW-1, F-LATTICE-1, F-UNPROJECT-1), `render` (ProjectionFrame, F-HIT-1), M1 lifecycle in `protocol`, single-output `output_connected` creating and binding one camera.
  - **adapter:** compositor/xdg-shell handlers, snapshot extraction, `SurfaceKey` registry, `convert` (F-RENDER-1).
  - **headless backend:** one virtual output, DummyRenderer.
- **Tests:** 32 core T0 and 7 T1 lockstep tests with a real probe client. Every pixel matches the presented surface at four output scales; xsnap/X-15, x6, xhit and the d5hit zoom-1 configuration are ported with their negative controls.
- **Enforcement:** `tools/verify-architecture` PASS.
  - **New rules:** `stage-single-writer`, `smithay-rejected-apis`, `smithay-geometry-only-in-convert`; unsafe scan now covers `tests/`.
  - **Dependencies:** dev-only `wayland-client`/`wayland-protocols` for the headless backend's T1 tests.
  - **Seeds:** 46 seeded controls.
- **Test efficacy:** new `tools/verify-test-efficacy` PASS (18/18 seeded behavioural defects caught; also in CI).
- **Not started:** multi-output, binding precedence, 2i (INC-2 / OB-49); input (INC-3/4); scale (INC-6); toplevel requests (INC-7, finding INC1-F01, Level C).
- **Registries:** unchanged (57 invariants, 20 `F-*`, OB-1…OB-51); checker PASS, self-test 18/18.
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT (at implementation):** independent INC-1 review. Superseded by the ratification entry above.

## PHASE F — INC-0 (implemented: `632b1d0`; evidence: evidence/phase-f/INC-0/; review: audit/phase-f-inc0-review/)
- **Scope:** ENFORCE, HEADLESS. The production workspace (`crates/`: core, wayland, backend-headless, backend-winit, ipc, bin) carries the authority **shape** only; no product behaviour. Enforcement-only crates live in `enforcement/`.
- **Pins:** `rust-toolchain.toml` 1.99.0; `Cargo.lock` tracked; `smithay = "=0.7.0"` from crates.io in the adapter; no patch (P1 stays experimental; OB-11 open).
- **Enforcement:** `tools/verify-architecture` PASS: DAG (normal, dev, build, target); syn-based source rules (2g, 2h, mint location, wayland names, unsafe policy, authority escape prevention across value/ref/mut/ptr/aliases); 26 negative probes with asserted diagnostics plus 1 positive control; 30 seeded negative controls plus 3 judge controls. CI: `.github/workflows/ci.yml` (foundation-docs, verify-architecture, workspace-build).
- **Independent Review & Corrective Patch:** Independent adversarial review confirmed compiler probes, unsafe policy, and workspace isolation, identifying findings INC0-F01 (reference getters) and INC0-F02 (dev-dependencies). Corrective patch resolved both findings with seeded regression controls.
- **Decision D Ratification:** OB-19 ratified `CLOSED` with explicit transfer of runtime 2i validation (`probe_sealed_key_steal` / `concurrent_output_key_collision`) to INC-2 / OB-49. `INV-CAMERA-3` and `INV-OUTPUT-2` remain mandatory without waiver. OB-36 ratified `CLOSED` with CI workflow execution verified.
- **Registries:** unchanged (57 invariants, 20 `F-*`, OB-1…OB-51); checker PASS, self-test 18/18.
- **LIVE_SYSTEM_TOUCHED:** no.
- **NEXT:** INC-1 only upon explicit owner authorization.

## PHASE E FINAL REVIEW (recorded: `cfb54f091bd2a19065a45b32662e8f0bbddd62fc`)
- **Verdict:** `PHASE E PASS — PHASE F MAY BEGIN` (audit/phase-e-final/VERDICT.md). Baseline reviewed: E1R seal `1ae1b736515b8c52c3e6ba7596b22c81f7098390`, tracking `e9d36732ac4fa9e934ee1b90bb696e9545c598ab`. Zero unresolved Level-A findings.
- **Architecture:** unchanged by the review and by this entry; registries as in E1R (57 effective invariants; 20 `F-*`; OB-1…OB-51).
- **Downstream gates (from the verdict, not new):** OB-11 before INC-5; OB-24 (G-OB24) before INC-10; OB-34 (G-OB34) before INC-14; OB-45 at INC-3 exit; OB-46 for physical rotation claims; G-MIRROR; G-OB48.
- **NEXT:** Phase F entry increment **INC-0** (closes OB-19, unconditional entry gate), only on explicit owner authorization. Not begun.

## PHASE E CORRECTIVE CHECKPOINT E1R (sealed: `1ae1b736515b8c52c3e6ba7596b22c81f7098390`)
- **E1R_SEAL:** `1ae1b736515b8c52c3e6ba7596b22c81f7098390`. Independent re-review: `E1R PASS — READY TO SEAL AND RE-ENTER INDEPENDENT PHASE E REVIEW` (audit/phase-e-e1r-review/); accepted E1R-N1 correction (no second corrective layer) and P1 as the minimal pinned framework correction.
- **Record:** audit/E1R/ (README, DISPOSITIONS, REPRODUCTION, INVARIANT_DIFF, D8_REAUDIT, REGRESSION).
- **Layer:** FOUNDATION_ARCHITECTURE §C12.E1R amends 6 invariants (INV-CAMERA-3, INV-INPUT-5, INV-OUTPUT-1, INV-OUTPUT-2, INV-SCALE-2, INV-SCALE-5); 57 effective invariants; 20 `F-*` (10 amended incl. new F-RECONF-1); OB-45…51; EE-1…8; W-SM-4c, W-SM-16; gates G-OB48, G-MIRROR.
- **Level A:** E-F2-01 and E-F2-02 corrected with regression evidence and negative controls; E-F2-02's correction depends on dependency patch P1 or an upstream fix (OB-11 criterion 3 open). **New Level A E1R-N1 (F-HIT-1 transform) found and corrected; under the E1R rules it blocks E1R pending review.**
- **Checker:** PASS; self-test 18/18.
- **NEXT:** INDEPENDENT FINAL PHASE E REVIEW OF THE SEALED E1R BASELINE. Phase F remains locked until that verdict passes and is accepted.

## Baselines
- Phase A evidence: `2c8c163784e260af4aaa8c1dc05c6281b0476120`
- Phase B seal: `4a0807ed721ae257e1f4b0e5287d1dfd8c1038c1`
- Phase C checkpoint 1: `d596416cf57877116e9cfbc8cf6c6fe25c001c67`
- Phase C checkpoint 2: `fc2dd68f843b1eb05828f4116f94e8bab4f478da`
- **Phase C checkpoint 3A:** `795800937a1e50e56587cd4080a73ce27d8a8b01` (hash recorded in `fd9a80c`)
- **Phase C seal (checkpoint 3B):** `615b0ed2f3631b698b0c54783c28f696ef4ff34a`

## PHASE D CHECKPOINT 2 — D7–D9 (sealed: `c0a1cee310e56b75d32cf30152a09975ff776700`)
- **Package:** 11 living documents. New: FOUNDATION_INVARIANTS, _OBJECT_MODEL, _STATE_MACHINES, _TEST_PLAN, _RISKS, _IMPLEMENTATION_PLAN. Living regions appended to ARCHITECTURE, EVIDENCE, BLOCKERS, DECISIONS and the LEDGER; historical text untouched.
- **Namespace map** in ARCHITECTURE §P0; 17 namespaces, one home each.
- **Registries:** 57 effective invariants; 19 `F-*`; 11 `PROV-*`; 44 `OB-*` (OB-36 checker CI; OB-37…41 PD-F1-10; OB-42…44 PD-F1-11); 66 risks; 114 evidence entries; 34 objects; 22 machines; 25 tests; 23 ledger entries; 18 increments.
- **Findings in D7:** PD-F1-11 (D-F1, applied); PD-F0-10 and PD-F0-11 (editorial ADR notes). No new D-F2.
- **Checker:** `tools/check-foundation-docs` PASS; `--self-test` 12/12 seeded defects detected.
- **D9:** 19/19 PASS. Open D-F2: PD-F2-06 (B), PD-F2-13 (C), PD-F2-15 (C), PD-F2-16 (C).
- **NEXT:** Phase E (independent adversarial review) only after explicit authorization.

## PHASE D CORRECTIVE CHECKPOINT 2R (sealed: `5827926782089dc8c6a1c203f844fbc879fce9be`)
- **Trigger:** D7–D9 stopped at PD-F2-18 (Level A). PD-F2-19 (Level C) and PD-F1-10 recorded at the same time (FOUNDATION_BLOCKERS.md § Phase D Checkpoint 2).
- **PD-F2-18 resolved:**
  - dedicated `OutputLifecycleToken` (INV-OUTPUT-2, new);
  - INV-CAMERA-1/3 and INV-OUTPUT-1 amended;
  - hardware-only descriptor; passive persisted lookup; no public restore API.
  - Evidence ER2-1: authority spike, 6 new probes and 3 new tests; `verify.sh` PASS.
- **PD-F2-19 resolved:** INV-SCALE-3/4 cover all spatial interactions. Evidence ER2-2 (`d5_model_2r`): window drag 39 → 1, with a negative control.
- **Registry:** 57 entries (§C12.R2, audit/2R/INVARIANT_DIFF_2R.md).
- **Clarifications at the seal (C12.R2.3, D-F0, non-semantic):**
  - `OutputLifecycleToken` lifecycle: exactly one per Core, returned once to the composition root, never re-issued, lifecycle operations only. Backed by the `verify.sh` 2g source check with a seeded negative control.
  - Device-bound (P7) exception: only when `h > H_max(E)`; never a damping bypass.
  - 300 ms remains provisional (OB-23).
- **PD-F1-10:** left open for D7.
- **NEXT:** D7–D9 only after explicit user authorisation.

## PHASE D CORRECTIVE CHECKPOINT 1R (sealed: `84ce620898445aaadb72f386ec3954e5bbfe2c2f`)
- **Baseline:** checkpoint 1 committed as `e8ec03a2ab36f315ae025723ec666b9ad278ac83` ("preserve Phase D falsification baseline"). This hash is recorded here and in audit/README.md, audit/1R/* and FOUNDATION_BLOCKERS.md.
- **Dispositions:** all 17 PD-F2 findings disposed of (FOUNDATION_BLOCKERS.md § Phase D Corrective Checkpoint 1R; audit/1R/CORRECTIONS_1R.md).
  - 13 resolved by architecture correction (A);
  - 1 resolved by another correction (B, PD-F2-11);
  - 3 now obligations: PD-F2-13 → OB-34, PD-F2-15 → OB-35, PD-F2-16 → OB-33;
  - PD-F2-06 jitter residual → OB-24 rescoped.
- **Re-evaluation:** 5 findings raised to Level A (PD-F2-03, -04, -05, -10, -17); all resolved.
- **Registry:** amended under provenance in FOUNDATION_ARCHITECTURE §C12.R1 (sealed C12 table unaltered). 14 invariants amended, 2 new (INV-INPUT-10, INV-PROTO-1); 56 entries.
- **Evidence:**
  - ER-1 xpopup_focus;
  - ER-2 xlock_grab;
  - ER-3 xmaximize (maximize and fullscreen);
  - ER-4 authority spike probes 2d/2e;
  - ER-5 d5_model_1r;
  - ER-6 satellite `-listenfd`;
  - ER-7 regression re-run.
  - Every experiment ran with its negative control, all as expected.
- **New Smithay quirks:** W-SM-14 and W-SM-15. **New ADR:** ADR-14.
- **Provisional constant:** T_settle candidate 300 ms (OB-23).
- **Explicit residual (NOT resolved, NOT validated):** the ±1 physical-pixel shape variation at lattice-reference switches during zoomed pan (ED-1, ED-2 F1). It is not a rendering guarantee. OB-24 keeps it open as a **pre-implementation decision for zoomed navigation** (candidate: gesture-pinned reference; visual acceptance T3/T4).
- **NEXT:** D7–D9 only after explicit user authorisation.

## PHASE D CHECKPOINT 1 (committed as e8ec03a)
- Input baseline verified (audit/README.md §1): seal resolves; `a3ae907` records it; tree was clean; 54-entry C12 registry normative; BLOCKERS pointer present; OB-19 unconditional; T3–T5/UP/TUNE open.
- D1 threat model; D2 22 state machines; D3 authority matrix (40 items, 6 searches); D4 coordinate matrix (16 domains, 26 conversions); D5 four-epoch audit; D6 FOUNDATION_DECISIONS.md (ADR-01…13).
- Findings: 17 D-F2 (A: 2, B: 6, C: 9), recorded in FOUNDATION_BLOCKERS.md § Phase D Architectural Findings; 9 D-F1; 9 D-F0; no D-F3. Registry NOT edited.
- New evidence: ED-1 d5hit (Smithay, 0 mismatches incl. k ∉ n/120), ED-2 d5_model (MODEL), ED-3/4/5 (source).
- NEXT: user disposition of PD-F2-01/02 (Level A) and the other D-F2s; commit of checkpoint 1 only on authorisation; D7–D9 not begun.

## COMPLETED_WORK (3B)
- C11: verification tiers T0–T5 (environment, simulated/real, invariants, tests, cost, trigger, evidence, meaning of failure, tier-honesty rule); OB-1…OB-29 all mapped; negative controls retained.
- C12: canonical invariant registry, 54 `INV-*` with owner, class, evidence, tests, tier and violation consequence; legacy → canonical map; supersessions marked (D-B2-I9 tolerance → INV-HIT-1; D-B8-SCALE wording → INV-SCALE-*).
- C13: implementation-readiness audit; rules promoted into normative text.
- C14:
  - performance methodology (no invented budgets);
  - idle contract (INV-IDLE-1);
  - allocation contract (INV-ALLOC-1);
  - scale-churn contract (INV-SCALE-4);
  - feedback damping SC-E/SC-D with model evidence (EC-22);
  - provisional tuning list.
- OB-19 formalised as the unconditional Phase F entry gate.
- C15 compatibility contract; C16 non-goals; physical obligations (C-PHYS).
- Authority reconstruction (C-AUTH): no dual writers.
- Final adversarial audit (25 scenarios): no contradictions between invariants; five under-specifications closed (AUD-4, -12/-13, -24, -25).
- Phase C gate evaluation: satisfied.

## FILES_CHANGED (committed in the seal)
- `docs/foundation/FOUNDATION_ARCHITECTURE.md` (3B sections)
- `docs/foundation/FOUNDATION_BLOCKERS.md` (normative-pointer note)
- `docs/foundation/CHECKPOINT.md`
- `docs/foundation/evidence/phase-c/cp3b-feedback-model-output.txt`
- `experiments/cp3a-scale-model/feedback.py`

## PROVISIONAL (C14.5)
H_cfg 3.0 · r 1.25 · σ 0.05 · P 1 s · SC-D decrease delay off · renderer filters Linear.

## OPEN, WITH DESTINATIONS
- OB-19 = Phase F entry gate (first production-workspace commit).
- All other OBs in C11.2 (T0–T5 / UP / TUNE / zoom-motion decision).

## LIVE_SYSTEM_TOUCHED
**No.** 3B ran only offline model scripts and documentation work; no clients, compositors or system changes.

## NEXT_ACTION
Phase D (falsification and document reorganisation) only after explicit user authorisation. Approval scope: the architecture is sufficiently specified to advance; it is **not** physically validated or production-ready. T3–T5 and upstream obligations remain authoritative future requirements.
