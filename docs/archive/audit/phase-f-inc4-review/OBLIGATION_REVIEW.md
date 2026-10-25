# INC-4 Review — Obligation Dispositions and Authority Audit

## 1. Obligations touched by INC-4

| OB | Canonical closure criterion | Evidence | Disposition |
|---|---|---|---|
| **OB-30** | PF-1…PF-7 pass on the production path; NEG-1, NEG-2, OBS-1 **fail as predicted**; no panic, no duplicate enter/leave, no stuck key | PF-1…7 pass; NEG-2/OBS-1 assert nothing predictive (F12); mutations I/J/K survive; stale-grab defects (F09) | **OPEN** (INC-4 exit blocked) |
| **OB-15** | no compositor repeat; output removal changes no keyboard state | repeat PASS; output removal with a popup grab changes keyboard routing (ATK-15) | **OPEN** |
| **OB-4** | `popup_done` in reverse creation order before the camera change; no panic | `popup_done` before any camera change, no panic — met literally; but the dismissal delivers a synthetic `wl_pointer.motion` during an acquired CameraPan (INV-INPUT-3, F05) and rule 1 (attached to OB-4 by INC3-F08) is not wired (F06) | **OPEN** |
| **OB-18** (popup part; owner of closure INC-7) | misbehaving-client tests; protocol-correct refusal; compositor survives | survives (no panic path); refusal does not dismiss (F08); PF-WSM7 vacuous | **OPEN** (popup part not satisfied; W-SM-6 part INC-7) |
| **OB-21** | T4 physical libinput: camera = start − Q(Σ), no drift | none in INC-4 (HEADLESS). Canonically **not** an INC-4 obligation (destination T4, deps INC-3/INC-16). Headless intake through `InputPort` is not physical libinput. | **OPEN — owner INC-16 / T4; requires owner authorization for physical hardware**. CHECKPOINT's "OB-21 (T4 intake in INC-4)" is a record error (F13) |
| OB-45 | (closed at INC-3) | t1_transform 7/7 under 1024; no regression | remains **CLOSED** |
| OB-3, OB-50, OB-31, OB-11, OB-47 | INC-5 | untouched; forward risk F10 (unset_grab motion under locks, W-SM-16) | **OPEN (INC-5)** |
| OB-10 | T3 toolkits (RC-1) | not addressable headless | **OPEN (T3)** |
| OB-16 | kick on input delivery | INV-INPUT-10 step 1 "kick first" not implementable before INC-8 | **OPEN (INC-8)** — DEFERRED, not tested |
| OB-32 | maximize/fullscreen answers (rest of INV-PROTO-1) | not in INC-4 | **OPEN (INC-7)** |
| OB-27 | activation grant | not in INC-4 | **OPEN (T3)** |

No obligation is closed by this review.

## 2. Authority / negative ownership

| Object | Canonical owner | Allowed mutators (canon) | Actual mutators at HEAD | Verdict |
|---|---|---|---|---|
| OBJ-25 keys | core intake | press decisions in core (INV-INPUT-1/-6) | adapter `keyboard.rs` via **pub** `Core::key_ownership_mut()` / pub `Ownership::press/release`; any crate holding `&mut Core` | **VIOLATED** (F01) |
| Physical key intake | backend → `PhysicalEvent` + `InputPort` → `Core::physical_input` | backend source holding the port | pub portless `WaylandState::keyboard_key`; headless `keyboard_key` calls the advisory `check_port` then the adapter; mutation T survives | **VIOLATED** (F01) |
| OBJ-22 ActiveElementFocus | core FocusModel | explicit actions, target loss | FocusModel (good) but triggers reachable through pub `Core::set_focus`, `Core::focus_next`, `WaylandState::set_focus` with no proof/port; the keyboard binding action is executed by the adapter | **WEAKENED** (F01) |
| ElementId | core stage (monotonic, never reused) | core | pub const `ElementId::new` — forgeable | Observation within F01 |
| OBJ-23 ProtocolSurfaceRoute | adapter (Smithay PopupGrab) | validated grab, client destruction, ordered preemption | adapter copy `popup_grab` diverges from Smithay's real grab state (stale after client/Smithay/zero-output endings) | **DUPLICATED STATE, DIVERGES** (F09) |
| OBJ-27 XKB / modifiers | Smithay `KeyboardHandle` only | Smithay | Smithay for keys **and** a backend snapshot in `PhysicalEvent::PointerButton` for buttons | **TWO AUTHORITIES** (F16) |
| `activated` | core decides, adapter applies | INV-PROTO-1 | adapter reads `core.focus().target()` at apply time (single source) | PASS |
| Serial validity | — (not canonically specified) | — | none | Observation (F18) |
| Core → Smithay calls | none allowed | — | none (core has no smithay dependency; verify-architecture DAG PASS) | PASS |
| Backend mutating Core state | only through ports/tokens | — | headless keyboard path reaches Core mutation through the adapter, port unchecked structurally | **VIOLATED** (F01) |

`verify-architecture` was not extended for any INC-4 boundary (no rule against adapter writes to OBJ-25, against portless physical intake, or against probe/NEG behaviour outside `probe_*` features).

## 3. Gaming-safety summary

Relative motion path unchanged by the diff (no new transform touches deltas) — PASS. Regressions/risks: Escape never reaches games (F04); synthetic absolute motion at CameraPan start (F05) and on every keyboard focus change with a grab (F10); key repeat/ownerless-release leaks (F03); two modifier authorities for the pan chord (F16). Nothing in INC-4 makes INC-5 impossible, but INC-5 inherits F10 (W-SM-16 under a lock) and F01 (a keyboard path that cannot mint `Authorized` for future keyboard-driven camera actions).
