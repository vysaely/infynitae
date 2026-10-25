# INC-4 Review — Independent Reconstruction, Integrity and Diff Audit

Reviewed HEAD `d5b7b95cbc910c48377c72ae94ccec2b0c05e140`; baseline `1ee520fa0079d45193f2a7274b31018e9276b0ed` (INC-3 ratified). Review date 2026-10-05.

## 1. Repository integrity

| Check | Observation | Result |
|---|---|---|
| Working tree | `git status --porcelain`: clean (ignored: `target/`, `experiments/*/target/`, `tools/__pycache__/`) | PASS |
| `git fetch origin` | no new objects | PASS |
| HEAD == origin/main | both `d5b7b95cbc910c48377c72ae94ccec2b0c05e140` | PASS |
| HEAD == *expected* INC-4 HEAD | review brief names `d5b7b95000572d45cfa2c8f8fb441f71dfbc9b3d`; that object **does not exist** (`git cat-file -t` fails). The real HEAD shares only the 7-char prefix `d5b7b95`. | **DISCREPANCY** (INC4-F15, Observation) — reviewed the commit actually at origin/main; owner should confirm the intended hash |
| Mapping commit | `f296a737c8ae521035eda90182625f25180cfbec` exists, parent of HEAD | PASS |
| Baseline ancestry | `1ee520fa…` is an ancestor of HEAD; range = 2 commits (`f296a73`, `d5b7b95`) | PASS |
| Historical seals / dossiers | Phase C/D/E seals listed in CHECKPOINT resolve; `audit/phase-e*`, `audit/phase-f-inc0…inc3-review/` present | PASS |
| Tags | repository carries no tags (seals are recorded by hash only) | Observation |

## 2. Canonical sources used (not the implementation report)

`FOUNDATION_IMPLEMENTATION_PLAN.md` §INC-4/§INC-5; `FOUNDATION_ARCHITECTURE.md` §C1 layer table (l.56), §C4.4 (K1–K9, X-10), §C4.5, §C4.6, §C12.R1 (INV-INPUT-10, INV-PROTO-1), CP2-1; `FOUNDATION_STATE_MACHINES.md` M3, M11, M12, M22; `FOUNDATION_OBJECT_MODEL.md` OBJ-22…OBJ-27; `FOUNDATION_INVARIANTS.md` INV-INPUT-1/3/6/7/8/10, INV-PROTO-1; `FOUNDATION_BLOCKERS.md` B6.2 (D-B6-BOUNDARY), B6.3, B6.5, W-SM-7/14/15/16, PD-F2-02; `FOUNDATION_DECISIONS.md` ADR-14; `FOUNDATION_TEST_PLAN.md` OB-4, OB-15, OB-18, OB-21, OB-30, TST-07, TST-20; harness evidence `experiments/b8-harness/src/{keys.rs,server.rs,bin/x10.rs,bin/xpopup_focus.rs}`; xdg-shell.xml (wayland-protocols 0.32.13) `xdg_popup.grab`.

## 3. Independent scope reconstruction

### IN SCOPE (canonical INC-4)
1. **M12 / K1–K9 / INV-INPUT-6/7** — key ownership decided at press by **core intake** (C1 table: Core owns "input intake, bindings, ownership table"; adapter only *applies* "keyboard forward/intercept"); OBJ-25 owner core intake, Smithay filter "executes only"; OBJ-27 XKB owned solely by Smithay, Core keeps no copy. Physical device events reach Core as `PhysicalEvent` submitted with `InputPort` (C1 table backend column).
2. **M11 / OBJ-22 / INV-INPUT-8** — ActiveElementFocus, single writer core FocusModel; writers: click-to-focus, focus-navigation bindings, shell command (no vocabulary until INC-11), activation grant (INC-12/OB-27 area), target loss → successor = **most recently focused live element** (C4.5). Never camera/visibility/pointer motion/protocol routing.
3. **OBJ-23 / INV-INPUT-10 / M3 grab rows** — popup grabs only for a valid grab whose root is the focused element, W-SM-7 precondition, W-SM-14 nested retention, ordered preemption `ungrab(All) → kb.unset_grab → ptr.unset_grab → set_focus`; refusal semantics per xdg-shell ("If the compositor denies the grab, the popup will be immediately dismissed").
4. **INV-PROTO-1 `activated` part only** — `activated` holds exactly on the ActiveElementFocus element.
5. **OB-4** — `popup_done` topmost first before the camera change on CameraPan acquisition (M3, C4.6); must not breach INV-INPUT-3.
6. **OB-15** — no compositor repeat; output removal changes no keyboard state (K8).
7. **OB-30** — PF-1…PF-7 on the production path **and** NEG-1, NEG-2, OBS-1 *fail as predicted*.
8. **OB-18 popup part** — misbehaving grab requests: protocol-correct refusal, compositor survives.
9. **D-B6-BOUNDARY rule 1** — transferred to INC-4 by the INC3-F08 adjudication (OB-4 history line): anchor output = output of the causing input event (grab serial, else seat's last input) if the parent is visible on it.
10. Exit tests TST-07 (x10 9 scenarios + K3-disabled NEG) and TST-20 (xpopup_focus).

### OUT OF SCOPE / DEFERRED (owner)
- Pointer constraints, reconciliation with locks, W-SM-4/4c/16 consequences, OB-11 decision → **INC-5**.
- Scale epochs → INC-6. Maximize/fullscreen/minimize answers of INV-PROTO-1, OB-32, OB-18 `ensure_configured` (W-SM-6) → **INC-7**.
- Kick on unobserved client (INV-INPUT-10 step 1 "kick first", X-10 case 8, OB-16) → **INC-8**.
- Shell focus command → INC-11. Activation grant / xdg-activation → OB-27 (T3) / later.
- Physical libinput intake, OB-21 (T4) → **INC-16 / T4**; not an INC-4 obligation (INC-4 is HEADLESS).
- Interaction *cancel* mechanics (M22 "cancel" row) were not introduced by INC-3; INC-4's K6 can only be exercised vacuously (finding INC4-F12).

### Canonical contradictions found (classified, not resolved)
- **C-1** CHECKPOINT (INC-3 ratification entry) says "OB-21 (T4 intake in INC-4)"; the canonical OB-21 row says destination T4, dependencies INC-3, INC-16, and INC-4's plan does not list OB-21. → Level C record inconsistency (INC4-F13).
- **C-2** C4.5 successor policy "most recently focused live element" vs. implementation fallback to z-order top when history has no live element; no canonical text defines the no-history case. → Level C (INC4-F11).
- **C-3** INV-INPUT-10 mandates pointer `unset_grab`, which in Smithay 0.7.0 always replays `motion` to the pending focus (W-SM-16). During a CameraPan acquisition this collides with INV-INPUT-3; under a lock (INC-5) it is W-SM-16. → Level C forward-risk (INC4-F10); the OB-4 path itself is a Level B (INC4-F05).
- **C-4** B6.4 assumes "user action on their element raises it (focus policy, C4)"; C4.5 lists no raise and INC-4 does not raise on click-to-focus. → Observation.
- **C-5** M3 lists `Mapped → grab(serial)` but xdg-shell (and Smithay `grab_popup`) require the grab *before* the popup's first commit. → Observation (wording).

## 4. Complete diff audit (`1ee520f..d5b7b95`, 26 files, +2812/−59)

| Area | Change | Assessment |
|---|---|---|
| core `input/keys.rs` (new) | `CANONICAL_BINDINGS` = x10 *scenario* bindings (Super+Enter FocusNext, Super+B Noop, **Esc Cancel (no modifier)**, LeftShift Noop) + pure resolver | Test fixtures hard-coded as product policy; Esc globally swallowed (INC4-F04) |
| core `input/focus.rs` (new) | FocusModel {target, history}; `set_focus`, `focus_next(z_order)`, `target_lost` with `.or_else(live.last())` | live set = `Stage::z_order`, which retains **unmapped** elements (INC4-F07); fallback non-canonical (INC4-F11) |
| core `input/mod.rs`, `ownership.rs` | `key_ownership: Ownership`; `Ownership::press/release` made **pub** | OBJ-25 externally writable (INC4-F01) |
| core `lib.rs` | pub `keys()`, `key_ownership()`, **`key_ownership_mut()`**, **`set_focus()`**, `focus_next()`, `resolve_key_binding()`, `check_port()` | authority-bearing mutators with no port; duplicate getters |
| core `stage.rs` | **pub const `ElementId::new`** | ElementIds forgeable outside core (Observation, part of INC4-F01) |
| core `entry.rs` | click-to-focus on client-owned press; `DismissPopupGrab(target)` on CameraPan acquisition | canonical intent; untested at T1 (click-to-focus) |
| core `protocol.rs` | `KeyboardFocus`, `ToplevelConfigure`, `DismissPopupGrab` effects; target loss on unmap/destroy; `popup_constraint_target(parent, causing_output)` | rule 1 never fed by the adapter (INC4-F06) |
| adapter `keyboard.rs` (new) | `WaylandState::keyboard_key` (pub, no port): intercept, **ownership decision**, OBJ-25 writes, binding actions, forward; runtime NEG setters | authority inversion (INC4-F01); K1/K2 dispatch defects (INC4-F03) |
| adapter `handlers.rs` | `grab()` with root/focus check, anvil-style keyboard/pointer checks, W-SM-14 | refusal leaves popup alive (INC4-F08) |
| adapter `outputs.rs` | apply `ToplevelConfigure`, `KeyboardFocus` (ordered preemption keyed on a stored `popup_grab`), `DismissPopupGrab` | stale `popup_grab` bookkeeping (INC4-F09); synthetic motion (INC4-F05) |
| adapter `pointer.rs` | grab helpers keep `smithay-pointer-only-in-adapter-pointer` | compliant |
| adapter `lib.rs` | `add_keyboard(200, 25)`; `popup_grab`, `k3_enforced`, `nested_grab_keep_pointer`, `ordered_preemption_strategy` **runtime** fields | NEG modes ungated in production (INC4-F02) |
| backend headless | `keyboard_key(state, port, …)`: `check_port` then adapter call | port check is advisory; bypassable (INC4-F01) |
| tests | `t1_keyboard.rs` (13), `t1_popup_focus.rs` (13), harness keyboard recorder | several vacuous oracles (INC4-F12) |
| tools | `verify-test-efficacy`: +5 controls, none removed | header string still says "(… INC-3)" (cosmetic) |
| tools | `verify-architecture`: **unchanged** — no INC-4 structural rule | new authority boundaries unenforced |
| docs | CHECKPOINT INC-4 entry; evidence dir | counts misreported (INC4-F14) |

Scope bleed into INC-5+: none in behaviour (no constraint code). Forward risks: INC4-F05/F10 (unset_grab motion), INC4-F01 (keyboard path outside Core cannot mint `Authorized` for future keyboard camera bindings).
