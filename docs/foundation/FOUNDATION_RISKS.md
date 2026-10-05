# Infynitae Native — Foundation Risks (effective register)

Status: **Phase D checkpoint 2 (D7) living document.** Canonical living home of the RISK namespace (FOUNDATION_ARCHITECTURE §P0).

Existing identifiers are preserved. Their historical records remain at the named origin:
- `RB-*`, `A-B1-1`, `NB-*`: BLOCKERS;
- `RC-*`, `A-C1`, `C-NB-2`: ARCHITECTURE;
- `T[ATCPRF]-*`: audit/D1, the falsification baseline.

Disposition vocabulary:
- `MITIGATED`: the architecture handles it; verification obligations may remain;
- `OBLIGATION`: open work with an `OB-*` destination;
- `ACCEPTED`: stated residual, accepted;
- `DEFERRED`: physical/tuning destination;
- `UPSTREAM`: dependency fix.

Mitigations through a PD finding name its effective outcome (FOUNDATION_BLOCKERS register). Evidence and obligation IDs resolve in FOUNDATION_EVIDENCE.md and FOUNDATION_TEST_PLAN.md.

Count: 66 risks.

<!-- living:RISK -->

### TA-1
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** An untrusted client moves a camera
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-01 → resolved (INV-CAMERA-1, INV-CAMERA-5). Evidence: ER-4, ER2-1
- **Obligations:** OB-19
- **Invariants:** INV-CAMERA-1, INV-CAMERA-4

### TA-2
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Time-driven camera change without a port
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-01 → resolved (INV-CAMERA-1, INV-CAMERA-5). Evidence: ER-4, ER2-1
- **Obligations:** OB-19
- **Invariants:** INV-CAMERA-1, INV-SHELL-1

### TA-3
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** A second writer of keyboard focus
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-02 → resolved (INV-INPUT-8, INV-INPUT-10). Evidence: ER-1
- **Obligations:** OB-18, OB-30
- **Invariants:** INV-INPUT-8

### TA-4
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Core mispredicts delivered pointer focus under client grabs → wrong W-SM-4a decision
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-02 → resolved (INV-INPUT-8, INV-INPUT-10). Evidence: ER-1
- **Obligations:** OB-3, OB-30
- **Invariants:** INV-INPUT-5, INV-OUTPUT-1

### TA-5
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Client becomes a Stage writer
- **Disposition:** MITIGATED
- **Mitigation / evidence:** holds under the effective architecture (D1). Evidence: EC-12 (X11 move ignored); B2.3
- **Obligations:** —
- **Invariants:** INV-STAGE-1, INV-XWAYLAND-1

### TA-6
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Port leaks into protocol state
- **Disposition:** ACCEPTED
- **Mitigation / evidence:** accepted residual: deliberate composition-root edit (C7.4). Evidence: EC-15
- **Obligations:** OB-19
- **Invariants:** INV-CAMERA-1, INV-ARCH-1

### TA-7
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Shell becomes an arbitrary mutator
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-07 → resolved (INV-SCALE-3, INV-SCALE-4). Evidence: ER-5, ER2-2
- **Obligations:** OB-23
- **Invariants:** INV-SHELL-1/2

### TA-8
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Activation steals focus or navigates
- **Disposition:** DEFERRED
- **Mitigation / evidence:** deferred (T1). Evidence: B10.7; W-SM-8
- **Obligations:** OB-27
- **Invariants:** INV-INPUT-9

### TA-9
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Fullscreen seizes an output while the user is elsewhere
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-10 → resolved (INV-PRES-1). Evidence: ER-3
- **Obligations:** OB-22, OB-32
- **Invariants:** INV-PRES-1

### TA-10
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Derived value written back as authority
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-03 → resolved (INV-COORD-5, INV-INPUT-5). Evidence: ER-2
- **Obligations:** OB-31
- **Invariants:** INV-STAGE-6

### TT-1
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Destruction order on client death breaks lifecycle
- **Disposition:** MITIGATED
- **Mitigation / evidence:** holds under the effective architecture (D1). Evidence: EB-5; EC-12 (satellite kill)
- **Obligations:** —
- **Invariants:** INV-STAGE-4

### TT-2
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Two scale evaluations in one batch → double renegotiation
- **Disposition:** MITIGATED
- **Mitigation / evidence:** holds under the effective architecture (D1). Evidence: AUD-25
- **Obligations:** —
- **Invariants:** INV-SCALE-5

### TT-3
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** `leave` after the output global is destroyed
- **Disposition:** MITIGATED
- **Mitigation / evidence:** holds under the effective architecture (D1). Evidence: B7.2
- **Obligations:** OB-3
- **Invariants:** INV-RENDER-3, INV-OUTPUT-1

### TT-4
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Pointer state stale after a non-pan view change
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-04 → resolved (INV-INPUT-5). Evidence: ER-2
- **Obligations:** OB-3, OB-14, OB-31
- **Invariants:** INV-INPUT-5, INV-CAMERA-4

### TT-5
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Kick ordering: callback completion after the request
- **Disposition:** MITIGATED
- **Mitigation / evidence:** holds under the effective architecture (D1). Evidence: EB-9
- **Obligations:** OB-16
- **Invariants:** INV-FRAME-3

### TT-6
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Fallback double-fires after a kick
- **Disposition:** MITIGATED
- **Mitigation / evidence:** holds under the effective architecture (D1). Evidence: AUD-24
- **Obligations:** —
- **Invariants:** INV-FRAME-3

### TT-7
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Fullscreen presentation start/end vs configure/ack
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-10 → resolved (INV-PRES-1). Evidence: ER-3
- **Obligations:** OB-22, OB-32
- **Invariants:** INV-PRES-1

### TT-8
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** W-SM-4a: explicit deactivate before focus change → duplicate `unlocked`
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-02 → resolved (INV-INPUT-8, INV-INPUT-10). Evidence: ER-1
- **Obligations:** OB-30
- **Invariants:** INV-INPUT-5

### TT-9
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Shell command concurrent with client destruction
- **Disposition:** MITIGATED
- **Mitigation / evidence:** holds under the effective architecture (D1). Evidence: AUD-21
- **Obligations:** —
- **Invariants:** INV-SHELL-2

### TC-1
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Render ≠ hit at an effective scale not of the form n/120
- **Disposition:** MITIGATED
- **Mitigation / evidence:** holds under the effective architecture (D1). Evidence: EC-20 (n/120 only); **ED-1** (k = 1080/651: 0 mismatches)
- **Obligations:** OB-14
- **Invariants:** INV-HIT-1, INV-RENDER-1

### TC-2
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Relative jitter while panning at zoom ≠ 1
- **Disposition:** OBLIGATION
- **Mitigation / evidence:** PD-F2-06 → obligation (INV-COORD-3). Evidence: ED-1, ER-5
- **Obligations:** OB-24
- **Invariants:** INV-RENDER-1, INV-CAMERA-5

### TC-3
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Pan speed at zoom ≠ 1
- **Disposition:** OBLIGATION
- **Mitigation / evidence:** PD-F2-06 → obligation (INV-COORD-3). Evidence: ED-1, ER-5
- **Obligations:** OB-21, OB-24
- **Invariants:** INV-COORD-3

### TC-4
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Q16/f64 range of derived values and i32 conversion
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F1-01 applied in D7 (F-LATTICE-1), PD-F1-02 applied in D7 (F-VIEW-1)
- **Obligations:** OB-6
- **Invariants:** INV-COORD-1, INV-COORD-2

### TC-5
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Unlisted rounding point
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-12 → resolved (INV-COORD-2). Evidence: ED-2
- **Obligations:** OB-6
- **Invariants:** INV-COORD-2

### TC-6
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Element drag across independent cameras
- **Disposition:** OBLIGATION
- **Mitigation / evidence:** PD-F2-13 → obligation (INV-STAGE-1)
- **Obligations:** OB-34
- **Invariants:** INV-STAGE-1, INV-COORD-3

### TC-7
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Lock point detaches from its surface
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-03 → resolved (INV-COORD-5, INV-INPUT-5). Evidence: ER-2
- **Obligations:** OB-20, OB-31
- **Invariants:** INV-COORD-5, INV-INPUT-5

### TC-8
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Zoomed render with the wrong damage-tracker scale
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F1-03 applied in D7 (F-RENDER-1)
- **Obligations:** OB-6
- **Invariants:** INV-RENDER-1

### TC-9
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Arrangement origin leaking into projection
- **Disposition:** MITIGATED
- **Mitigation / evidence:** holds under the effective architecture (D1). Evidence: ED-2 (D5 uses non-zero origins; projection independent of them)
- **Obligations:** —
- **Invariants:** INV-COORD-4

### TP-1
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Pan visible to clients as configure or input
- **Disposition:** MITIGATED
- **Mitigation / evidence:** holds under the effective architecture (D1). Evidence: SC1, SC4, X-8; ED-2 epoch 2 (0)
- **Obligations:** —
- **Invariants:** INV-CAMERA-4

### TP-2
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Stuck key or modifier across focus changes
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-02 → resolved (INV-INPUT-8, INV-INPUT-10). Evidence: ER-1
- **Obligations:** OB-10, OB-30
- **Invariants:** INV-INPUT-6, INV-INPUT-7

### TP-3
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Confinement violated by Arrangement crossing
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-05 → resolved (INV-INPUT-5). Evidence: ER-5
- **Obligations:** OB-14, OB-31
- **Invariants:** INV-INPUT-5

### TP-4
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** `suspended` derived from camera visibility via an output-power reason
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-14 → resolved (INV-FRAME-6)
- **Obligations:** —
- **Invariants:** INV-FRAME-6

### TP-5
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Never-activated oneshot lock destroyed on leave
- **Disposition:** UPSTREAM
- **Mitigation / evidence:** deferred (UP). Evidence: X-12 (defect reproduced)
- **Obligations:** OB-11
- **Invariants:** INV-INPUT-5

### TP-6
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Popup grab chain panics (`grab_popup` assert)
- **Disposition:** DEFERRED
- **Mitigation / evidence:** deferred (T1). Evidence: W-SM-7 (source)
- **Obligations:** OB-18
- **Invariants:** INV-FAIL-1

### TP-7
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Satellite X11 menus misplaced
- **Disposition:** DEFERRED
- **Mitigation / evidence:** deferred. Evidence: C6-A1 (assumption)
- **Obligations:** OB-9
- **Invariants:** INV-XWAYLAND-1

### TR-1
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Scale renegotiation storm via non-gesture camera paths
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-07 → resolved (INV-SCALE-3, INV-SCALE-4). Evidence: ER-5, ER2-2
- **Obligations:** OB-23
- **Invariants:** INV-SCALE-4

### TR-2
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Unrenderable buffer from an output scale above the device bound
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-08 → resolved (INV-SCALE-2, INV-SCALE-5). Evidence: ED-2
- **Obligations:** OB-13
- **Invariants:** INV-SCALE-2

### TR-3
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Starvation of a FIFO-blocked client off-screen
- **Disposition:** DEFERRED
- **Mitigation / evidence:** deferred (T5). Evidence: EB-9
- **Obligations:** OB-5, OB-16
- **Invariants:** INV-FRAME-3

### TR-4
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Wakeups while idle
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F1-06 applied in D7 (OBJ-05)
- **Obligations:** OB-44
- **Invariants:** INV-IDLE-1

### TR-5
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Off-ladder hint after hotplug; renegotiation without ρ change
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-08 → resolved (INV-SCALE-2, INV-SCALE-5). Evidence: ED-2
- **Obligations:** OB-13
- **Invariants:** INV-SCALE-2, INV-SCALE-4

### TR-6
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Extra renegotiation at map
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-09 → resolved (INV-SCALE-5). Evidence: ED-2
- **Obligations:** OB-13
- **Invariants:** INV-SCALE-5

### TR-7
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Hot-path allocation
- **Disposition:** DEFERRED
- **Mitigation / evidence:** deferred (T0). Evidence: C14.3 design
- **Obligations:** OB-42
- **Invariants:** INV-ALLOC-1

### TF-1
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Output removal during constraint + grab
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-02 → resolved (INV-INPUT-8, INV-INPUT-10); PD-F2-03 → resolved (INV-COORD-5, INV-INPUT-5). Evidence: ER-1, ER-2
- **Obligations:** OB-3, OB-30, OB-31
- **Invariants:** INV-OUTPUT-1, INV-INPUT-5

### TF-2
- **Origin:** D1 (P1) @ e8ec03a
- **Risk:** Recovery patches derived state instead of rebuilding
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F1-08 applied in D7 (F-SCALE-2)
- **Obligations:** —
- **Invariants:** INV-FAIL-1

### TF-3
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Orphaned pan keeps mutating an unbound camera
- **Disposition:** MITIGATED
- **Mitigation / evidence:** PD-F2-11 → resolved (INV-CAMERA-5, INV-OUTPUT-1). Evidence: ER-4
- **Obligations:** OB-3
- **Invariants:** INV-OUTPUT-1, INV-CAMERA-3

### TF-4
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Satellite restart breaks `DISPLAY` for running children
- **Disposition:** OBLIGATION
- **Mitigation / evidence:** PD-F2-16 → obligation (INV-XWAYLAND-1). Evidence: ER-6
- **Obligations:** OB-9, OB-33
- **Invariants:** INV-XWAYLAND-1

### TF-5
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Shell restart loses truth
- **Disposition:** OBLIGATION
- **Mitigation / evidence:** PD-F2-15 → obligation (INV-STAGE-4)
- **Obligations:** OB-35
- **Invariants:** INV-SHELL-2

### TF-6
- **Origin:** D1 (P2) @ e8ec03a
- **Risk:** Corrupt persistence partially applied
- **Disposition:** DEFERRED
- **Mitigation / evidence:** deferred (Level D). Evidence: C8
- **Obligations:** OB-43
- **Invariants:** INV-FAIL-1

### RB-1
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** `Window::geometry()` re-clamps dynamically
- **Disposition:** OBLIGATION
- **Mitigation / evidence:** W-SM-9 accepted; observe real CSD clients
- **Obligations:** OB-17
- **Invariants:** INV-STAGE-2

### RB-2
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** `grab_popup` panics on root mismatch
- **Disposition:** MITIGATED
- **Mitigation / evidence:** W-SM-7 precondition check (INV-INPUT-10 refuses invalid grabs); ER-1 no panic
- **Obligations:** OB-18
- **Invariants:** INV-FAIL-1

### RB-3
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** `ensure_configured` opt-in
- **Disposition:** MITIGATED
- **Mitigation / evidence:** W-SM-6 `ensure_configured` on every role commit
- **Obligations:** OB-18
- **Invariants:** INV-STAGE-4

### RB-4
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** Arbitrary destruction order on client death
- **Disposition:** MITIGATED
- **Mitigation / evidence:** idempotent, order-independent destroy (EB-5, EC-12)
- **Obligations:** OB-18
- **Invariants:** INV-STAGE-4

### RB-5
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** Smithay membership helpers wrong for CSD/popups
- **Disposition:** MITIGATED
- **Mitigation / evidence:** own per-surface membership (F-MEMBER-1); X5, ED-1
- **Obligations:** OB-26
- **Invariants:** INV-RENDER-3

### RB-6
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** Default compositor v5
- **Disposition:** MITIGATED
- **Mitigation / evidence:** W-SM-3 `new_v6`
- **Obligations:** OB-19
- **Invariants:** INV-ARCH-3

### RB-7
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** MaxGesture memory cost (2.25× while multi-observed)
- **Disposition:** ACCEPTED
- **Mitigation / evidence:** buffer cost while multi-observed accepted (CP2-Q1 decision); bounded by H_max (PROV-1)
- **Obligations:** OB-12
- **Invariants:** INV-SCALE-1, INV-SCALE-2

### RB-8
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** Fallback P wrong for games or software clients
- **Disposition:** DEFERRED
- **Mitigation / evidence:** P is configuration (PROV-4); X-7 in T5
- **Obligations:** OB-5, OB-8
- **Invariants:** INV-FRAME-3

### RB-9
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** The kick completes callbacks for all unobserved surfaces of a client
- **Disposition:** ACCEPTED
- **Mitigation / evidence:** kick bounded by request/input rate (PD-F0-05)
- **Obligations:** OB-16
- **Invariants:** INV-FRAME-3

### RB-10
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** W-SM-4b: never-activated oneshot lock destroyed on pointer leave
- **Disposition:** UPSTREAM
- **Mitigation / evidence:** W-SM-4b reproduced (X-12); E1R: W-SM-4c reproduced (EE-2), candidate patch P1 validated (EE-2, EE-7); upstream fix or patch decision
- **Obligations:** OB-11, OB-47
- **Invariants:** INV-INPUT-5

### RB-11
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** Keyboard key ownership not yet verified
- **Disposition:** MITIGATED
- **Mitigation / evidence:** K-rules verified by X-10 (EC-6) incl. negative control
- **Obligations:** OB-15, OB-30
- **Invariants:** INV-INPUT-6, INV-INPUT-7

### RB-12
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** Interaction during output removal not yet verified
- **Disposition:** MITIGATED
- **Mitigation / evidence:** design + ER-2 (harness); production test pending
- **Obligations:** OB-3, OB-31
- **Invariants:** INV-OUTPUT-1

### A-B1-1
- **Origin:** Phase B §11 @ 4a0807e
- **Risk:** Zoom anti-jitter
- **Disposition:** OBLIGATION
- **Mitigation / evidence:** zoom anti-jitter: render/hit agreement intact (ED-1); ±1 px shape variation open
- **Obligations:** OB-24
- **Invariants:** INV-RENDER-1, INV-COORD-3

### RC-1
- **Origin:** Phase C CP2 @ fc2dd68
- **Risk:** client-side destruction of the keyboard-focused surface: protocol-implicit ordering
- **Disposition:** OBLIGATION
- **Mitigation / evidence:** validated for GTK4 (EC-21); X-10 K-6 (EC-6)
- **Obligations:** OB-10
- **Invariants:** INV-INPUT-7

### RC-2
- **Origin:** Phase C CP3A @ 795800937a
- **Risk:** client resizing in response to scale hints (feedback loop)
- **Disposition:** MITIGATED
- **Mitigation / evidence:** one-shot fit removed the re-fit loop (ER-5 R3); epochs + P7 (ER-5 R2, ER2-2)
- **Obligations:** OB-28
- **Invariants:** INV-SCALE-3, INV-SCALE-4

### A-C1
- **Origin:** Phase C C-A @ d596416
- **Risk:** surface-tree snapshot extraction through public Smithay API only
- **Disposition:** OBLIGATION
- **Mitigation / evidence:** harness proof (EC-8, ED-1); not yet on the production adapter
- **Obligations:** OB-26, OB-1
- **Invariants:** INV-COORD-6

### C-NB-2
- **Origin:** Phase C C-B @ d596416
- **Risk:** a deliberate composition-root edit passes a port or token into protocol code by parameter
- **Disposition:** ACCEPTED
- **Mitigation / evidence:** type-level guard against storage (E0277), source rules in OB-19; the residual is a conspicuous reviewed edit
- **Obligations:** OB-19
- **Invariants:** INV-CAMERA-1, INV-OUTPUT-2

### NB-12
- **Origin:** Phase B B13.2 @ 4a0807e
- **Risk:** harness ≠ production adapter: Phase B–D evidence may not transfer to the production seam
- **Disposition:** OBLIGATION
- **Mitigation / evidence:** harness-parity exit criteria for every PRODUCT increment (FOUNDATION_TEST_PLAN §4); Slice 1 (INC-1) tests the seam first
- **Obligations:** OB-26, OB-30, OB-31, OB-32
- **Invariants:** INV-ARCH-1

<!-- /living -->
