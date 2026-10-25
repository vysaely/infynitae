# INC-4 Re-Review — Obligations, Level C Adjudications, INC-3 Gate, Safety

## 1. Obligations

| OB | Disposition | Reason |
|---|---|---|
| OB-4 | **OPEN** | `popup_done` before the camera change and no pointer replay: met. Keyboard focus left on the dismissed popup (INC4R-01) |
| OB-15 | **OPEN** | repeat PASS; output removal (with reconnect) changes popup grab semantics (R12) |
| OB-18 (popup part) | **OPEN** pending ratification — refusal semantics now met (W-SM-7 PASS); W-SM-6 part INC-7 |
| OB-30 | **OPEN** | PF-1…7 pass, NEG-2/OBS-1 fail as predicted; but INC4R-01, R09b and survivors N08/N09 |
| OB-21 | **OPEN** — owner INC-16 / T4; physical libinput requires owner authorization (headless intake ≠ libinput) |
| OB-45 | CLOSED (no regression) |

## 2. Level C adjudications

| ID | Original conflict | Corrective decision | Canonical support | Weakened? | Implementation matches? | Future owner | Verdict |
|---|---|---|---|---|---|---|---|
| F10 | INV-INPUT-10 pointer `unset_grab` replays motion | "standard behaviour", isolated under CameraPan, deferred to W-SM-16/INC-5 | none recorded; self-adjudicated by the implementer | no text weakened | pan isolated (R10 PASS); focus-change motion persists | INC-5 named | **NOT ADJUDICATED** — needs owner decision (INC4R-10) |
| F11 | no-history successor | Unfocused | C4.5 literal ("most recently focused live element") | no | yes (R06) | — | **RESOLVED** |
| F13 | OB-21 attribution | INC-16 / T4 | OB-21 canonical row | no | CHECKPOINT updated — but by editing historical entries in place | INC-16 | **RESOLVED** (record issue INC4R-12) |
| F17 | parent unmap popups | dismiss on unmap/destroy | M3 row already canonical | no | yes (ATK-18; N14 detected) | INC-4 | **RESOLVED** |

## 3. INC-3 regression gate — **PASS**
All INC-3 suites unchanged and passing (t1_input 17, t1_adversarial 13, t1_transform 7 incl. `inc3_f09_bounded_resource_cycles`, t1_outputs 9, t1_parity 6, t1_slice 4); every INC-3 efficacy control detected; `[5b]`/`[5c]` INV-INPUT-3 bypass and port-binding checks pass. CameraPan with a popup: no client events during the pan, exactly the control's single true-position event at the end (R10). Zero-output grab preserved under preemption and removed by the first output (R11). INC3-F06 isolation code unchanged since the live concurrency check of the first review (verified by diff; not re-run concurrently). The modifier union in `pointer_button` is an INC-4 change, not an INC-3 regression.

## 4. Resource stress — **bounded**
R17 (one Lockstep, 150 cycles, nested distinct-serial grabs, pan-with-popup, key repeats, focus_next, unplug/replug, destroy): fds 10→10, focus history 1, owned keys 0, owned buttons 0, elements 1, no residual grabs. Shipped RES-1 rebuilds the Lockstep each cycle and measures fds only — weaker than R17. Protocol violations accumulate (600), tracked as INC4R-01, not as a resource leak.

## 5. Safety / scope confirmations
Production implementation untouched (mutations only in disposable worktrees, all reverted; `git status` of the repo shows only this untracked dossier). No live compositor, DRM/KMS, VT switch, package, boot or reboot action. Old Arch partition untouched. INC-5 not started.
