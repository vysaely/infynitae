# Phase F Increment 4 (INC-4) — Independent Adversarial Review Verdict

- **Reviewed commit:** `d5b7b95cbc910c48377c72ae94ccec2b0c05e140` (= origin/main). The brief's expected hash `d5b7b95000572d45cfa2c8f8fb441f71dfbc9b3d` does not exist (F15).
- **Pre-INC-4 baseline:** `1ee520fa0079d45193f2a7274b31018e9276b0ed` (INC-3 ratified); mapping commit `f296a737c8ae521035eda90182625f25180cfbec`.
- **Review date:** 2026-10-05. Production implementation not modified; review artifacts untracked.

## 1. Definitive verdict

```
INC-4 FAIL — ARCHITECTURAL CORRECTION REQUIRED
```

INC-4 delivers working happy-path behaviour for K3–K5, K9, PF-1…PF-7 and `activated`, and the whole verification matrix is green. It nevertheless fails on one authority finding that requires redesigning the keyboard entry path (F01), and on eleven Level-B defects, several of them reproduced on the production path with client-wire oracles (Esc swallowed, key leaks, focus on unmapped elements, synthetic pointer motion during an acquired CameraPan, keyboard route changed by output removal, refused grabs left alive, rule 1 not wired). 14 of 29 independent mutations survive the shipped suite, including every perturbation of the INV-INPUT-10 order. Green CI does not change this.

## 2. Findings catalog

### Level A — 1
- **INC4-F01 — Keyboard intake bypasses the authority boundary.** Physical keys never enter `Core::physical_input(&mut InputPort, PhysicalEvent)`. The backend calls a public, portless `WaylandState::keyboard_key` after an advisory `Core::check_port`. The adapter decides K1/K3 ownership and writes core-owned OBJ-25 through the new public `Core::key_ownership_mut()` (and the now-public `Ownership::press/release`). It also runs binding actions. Focus mutators `Core::set_focus`, `Core::focus_next` and `WaylandState::set_focus` are public with no proof, and `ElementId::new` is now public. This contradicts the C1 layer allocation (Core owns "input intake, bindings, ownership table"; the backend submits `PhysicalEvent` with `InputPort`; the adapter only *applies* forward/intercept), OBJ-25 ("Forbidden writers: Smithay filter (executes only)") and INV-INPUT-1 (owner core intake). Mutation T (port check removed) survives. No `verify-architecture` rule covers any of this. *Correction:* a keyboard `PhysicalEvent` through `physical_input`; core intake decides and records ownership and returns effects; the adapter executes `input_intercept`/`input_forward` as effects; authority-bearing mutators made crate-private; structural rules and compile probes added.

### Level B — 11
- **INC4-F02 — Negative-control modes ship as public runtime switches.** `set_k3_enforced`, `set_preemption_strategy(Naive|PointerFirst)` and `set_nested_grab_keep_pointer` disable INV-INPUT-6/10 and W-SM-14 at runtime in production. INC-3's convention is `probe_*` cargo features with hygiene checks. Because the tests set these switches explicitly, the production defaults are unprotected (mutations V, AB survive).
- **INC4-F03 — K1/K2 dispatch defects.** A repeated press of a compositor-owned key is forwarded (ATK-02; `Ownership::press` result ignored). A release with no recorded press is forwarded, which is a protocol must-not (ATK-04; Smithay `input_forward` does not filter, keyboard/mod.rs:1013-1043).
- **INC4-F04 — Harness scenario bindings hard-coded as product policy.** `CANONICAL_BINDINGS` copies the x10 fixtures, so plain **Escape is never delivered to any client** (ATK-01) and Super+B is swallowed for a no-op. Canonical documents define no product bindings.
- **INC4-F05 — OB-4 dismissal injects pointer events into an acquired CameraPan.** `ptr.unset_grab` replays `motion` (pointer/mod.rs:763-816). The client receives `motion 45,40` at the pan press (ATK-09), breaching INV-INPUT-3 and INC-3's stop condition.
- **INC4-F06 — D-B6-BOUNDARY rule 1 not on the production path.** The only caller passes `causing_output = None` (`handlers.rs:69`), so the INC3-F08 transfer is undischarged. The core test is `if`-guarded, and mutation P survives.
- **INC4-F07 — Focus can target unmapped elements.** `Stage::z` keeps unmapped elements, so `focus_next` (ATK-05) and the target-loss successor (ATK-06) focus unmapped toplevels, which then receive `wl_keyboard.enter` and `activated`. Unmap-of-focused (mutation W) is untested.
- **INC4-F08 — Refused popup grabs are not dismissed.** This violates xdg-shell ("If the compositor denies the grab, the popup will be immediately dismissed") and OB-18's protocol-correct refusal (ATK-11). PF-WSM7 is vacuous, and mutations N and L survive.
- **INC4-F09 — Adapter popup-grab copy diverges from Smithay; preemption hits unrelated grabs.** Preemption tears down the zero-output grab at zero outputs (ATK-12) and a client implicit button grab (ATK-14). Output removal moves the keyboard from the grabbing popup to the root without `popup_done` (ATK-15), which violates K8/OB-15 and xdg-shell's rule that the topmost grabbing popup keeps keyboard focus.
- **INC4-F12 — Verification does not meet OB-30 / TST-07 / TST-20.**
  - NEG-2 and OBS-1 do not assert the predicted failures (ATK-16/17 prove those failures are observable), and F-4 is never computed. Mutations I, J and K survive.
  - No test asserts `activated` (Q, R survive). There is no T1 click-to-focus test (U survives). Mutation F survives.
  - The KV-2/F-6 oracles are circular through production ownership, and K6 is exercised without any interaction.
- **INC4-F14 — Evidence and CHECKPOINT misreport counts.** They state "106 tests, 68/68 controls"; the actual figures are 151 tests and 83 controls (81 seeded + 2 judge), and the committed evidence files themselves show 151 and 81. No coverage was lost.
- **INC4-F16 — Two modifier authorities.** CameraPan reads a backend-supplied modifier snapshot while keys use Smithay XKB. With Super physically held, no pan is acquired (ATK-13). INC3-F01's scaffolding was meant to end at INC-4.

### Level C — 4
- **INC4-F10:** INV-INPUT-10 requires pointer `unset_grab`, which in Smithay always synthesizes `motion`. Every grab preemption therefore sends a motion (ATK-10), and under a lock this becomes W-SM-16. This needs adjudication before INC-5.
- **INC4-F11:** the successor fallback to the top of the z-order when no focus history exists (ATK-07) is not the canonical C4.5 policy (most recently focused live element); the no-history case is undefined.
- **INC4-F13:** CHECKPOINT says "OB-21 (T4 intake in INC-4)". Canonically OB-21 belongs to T4 / INC-16 and is not an INC-4 obligation.
- **INC4-F17:** the M3 row "parent unmapped/destroyed → `popup_done` for the tree" is not implemented for unfocused parents (ATK-18), and no increment owns it explicitly.

### Observations — 6
- **INC4-F15:** the expected-HEAD hash in the review brief does not exist.
- **INC4-F18:** there is no serial-provenance validation, so wrong-client and stale serials are accepted. Smithay offers none; the requirement that the root be focused mitigates this.
- **INC4-F19:** `activated` configures are sent before the preemption in the same batch, and every transition sends an unconditional `send_configure`.
- **INC4-F20:** B6.4 assumes that focus raises the element; INC-4 does not raise on focus.
- **INC4-F21:** INC3-F09 and INC3-F06 remain fixed. RES-1 shows 40 INC-4 lifecycle cycles with bounded descriptors (4 → 4).
- **INC4-F22:** the `verify-test-efficacy` header string still names INC-3 as the latest increment.

Pre-existing open: INC1-F01 (Level C, INC-7).

**Open counts after this review: Level A 1, Level B 11, Level C 4 (+1 pre-existing INC1-F01).**

## 3. Obligations
OB-30, OB-15, OB-4 and the OB-18 popup part stay **OPEN**. OB-21 stays **OPEN** (T4 / INC-16; physical intake needs owner authorization). OB-45 stays CLOSED. Nothing is closed by this review. Details are in OBLIGATION_REVIEW.md.

## 4. Verification matrix (scratch worktree of HEAD, `ulimit -n 1024`)

| Command | Result |
|---|---|
| `cargo check --workspace --locked` | PASS |
| `cargo build --workspace --locked` | PASS |
| `cargo test --workspace --locked` | PASS, **151** (core 69; T1 82: adversarial 13, input 17, keyboard 13, outputs 9, parity 6, popup_focus 13, slice 4, transform 7) |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `./tools/verify-architecture` | PASS. Section [4] 13 structural rules; [5] 32 ok lines (compile probes plus a positive control); [5b] 4; [5c] 5; [6] 58 ok lines (seeded controls, 2 judge controls, tree identity); 1 DEFERRED (shell_command, INC-11). **No INC-4 rules added.** |
| `./tools/verify-test-efficacy` | PASS, 81/81 seeded DETECTED + 2 judge = 83 |
| `./tools/check-foundation-docs` | PASS |
| `./tools/check-foundation-docs --self-test` | PASS (every seeded defect detected) |
| `./experiments/c1-authority-spike/verify.sh` | PASS |
| `./tools/verify-development-environment` | PASS |
| GitHub Actions run 37344638073 (push, exact HEAD) | foundation-docs, verify-architecture and workspace-build all success; 151 tests; 81 DETECTED |

## 5. Required action before any re-review
1. Redesign keyboard intake to close F01: `PhysicalEvent` key variant through `physical_input`, core-decided ownership, effect-driven intercept/forward, crate-private mutators, and new structural rules and compile probes. Record this as an architecture correction under change control.
2. Fix F02–F09 and F16, gating negative controls behind `probe_*` features.
3. Rebuild the T1 oracles to resolve F12: independent held-key model, NEG-2/OBS-1 asserting the predicted failures, F-4, `activated`, click-to-focus, refusal, zero-output and output-removal-with-popup cases. Make permanent efficacy controls for mutations F, I, J, K, N, P, Q, R, T, U, V, W and AB.
4. Correct the evidence and CHECKPOINT counts (F14), and adjudicate the Level-C findings F10, F11, F13 and F17 (F10 before INC-5).
5. This dossier stays unchanged. Corrective work goes in a separate corrective record.

INC-5 is **not** authorized by this review.
