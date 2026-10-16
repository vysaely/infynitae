# Infynitae Native — Foundation Test Plan

Status: **Phase D checkpoint 2 (D7) living document.** Canonical living home of `OB-*` (complete obligation registry), the verification tiers `T0–T5` and the test inventory `TST-*` (FOUNDATION_ARCHITECTURE §P0).

Historical obligation records stay unchanged as provenance: ARCHITECTURE CP3A-4, C11.2, the OB-19 section, C12.R1.3, C12.R2.2, and audit/1R CORRECTIONS_1R.

Every other living document cites `OB-*` by ID only.

## 1. Verification tiers

**Tier-honesty rule (unchanged from C11.1).** A tier claims only what it physically represents:
- T0/T1 never claim GPU, timing, visual, toolkit, game or device behaviour.
- T2 never claims DRM/KMS behaviour.
- T3 results are per toolkit version, never universal.
- Harness (HARNESS) and model (MODEL) evidence never closes a PRODUCT obligation by itself.
- Every evidence record states its tier and category (FOUNDATION_EVIDENCE.md).

<!-- living:TIER -->
### T0 — Pure core and enforcement
`cargo test` on the core crate (no Smithay, no runtime) plus `verify-architecture` and `tools/check-foundation-docs`. Covers COORD, CAMERA authority, STAGE identity/lifecycle, SCALE/FRAME/INPUT policy decisions, ARCH, and the document package. Every commit; blocks merge. A failure means a domain rule, an authority boundary or the documentation authority is broken.

### T1 — Lockstep headless integration
Production core + production Smithay adapter + headless backend (virtual outputs and vblank, injected `PhysicalEvent`s, real output connect/disconnect through `OutputLifecycleToken`), with recording probe clients over socketpairs. Covers RENDER order/edges (DummyRenderer), HIT, membership, INPUT, constraints, FRAME, PROTOCOL responses, SHELL IPC, FAIL. Every commit; blocks merge. It never claims GPU, timing or toolkit behaviour.

### T2 — Nested
Infynitae on the winit backend inside the developer's session (execution class NESTED, per-run authorization). Covers the real GLES renderer, damage, host frame timing and basic input. On demand. Proves nothing about DRM/KMS.

### T3 — Real clients and toolkits
GTK3/4, Qt6, Firefox, Chromium/Electron, kitty, SDL, xwayland-satellite with X11 apps, against the headless (real-time) or nested backend. Results are per toolkit version. PHYSICAL category; separate authorization.

### T4 — Physical hardware
DRM/KMS + libinput + real monitors (target EC-23) for presentation timing, VRR, input devices, hotplug, measured performance and idle (C14 procedures). Execution class EXCLUSIVE_HARDWARE; separate explicit authorization; never casually during a working session.

### T5 — Games, latency, long duration
Games (Wine/Proton), input-to-photon latency, 24 h stability, fuzzed clients on T4 hardware. EXCLUSIVE_HARDWARE; separate authorization.
<!-- /living -->

## 2. Obligation registry (canonical `OB-*`)

Destination vocabulary:
- `T0`…`T5`: verification tier;
- `UP`: upstream;
- `TUNE`: provisional value, frozen by measurement;
- `DECISION`: architecture change control;
- `PHASE-F-ENTRY-GATE`;
- `CI`;
- `GATE-C`: historical Phase C gate.

Status is `OPEN` or `CLOSED`. An obligation whose destination is T3/T4/T5/TUNE/UP/DECISION cannot be `CLOSED` without the evidence its pass criterion names. **No Phase D activity closes any obligation.**

<!-- living:OB -->
### OB-1
- **Status:** CLOSED
- **Item:** Multi-level popups (CR-1 residual). Test/decision: nested popup chain: order vs xdg-shell stacking (Smithay's except W-SM-20), positions vs Smithay render; hit-test vs presented (order criterion amended by FOUNDATION_ARCHITECTURE §C12.F8)
- **Owner:** core origin(), adapter extraction
- **Evidence available:** one popup level verified (EC-8/9/20); INC-7 production: T1 `ob1_nested_popup_chain_matches_what_is_presented` (three-level chain, sibling, subsurface, scale 1 and 1.25; 0 mismatches against the hand-derived truth, Smithay positions, xdg-shell order per C12.F8, per-pixel hit-test), `a_nested_popup_is_constrained_like_a_first_level_one`, `a_newer_popup_is_stacked_above_every_older_one`
- **Pass criterion:** 0 mismatches
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend) porting xsnap/xhit to nested popup chains
- **Dependencies:** INC-1
- **Blocks:** INC-7 exit (popups)
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** Ratified CLOSED at INC-7 exit (owner, 2026-10-14) on `99c24cd`; the order criterion is xdg-shell's (FOUNDATION_ARCHITECTURE §C12.F8), Smithay's except W-SM-20.

### OB-2
- **Status:** CLOSED
- **Item:** Input-region subtraction, general. Test/decision: multi-rect, interleaved add/subtract; random property test
- **Owner:** core hit-test
- **Evidence available:** single-hole fall-through verified (EC-20); INC-1 production: T0 `ob2_region_equals_reference_evaluator` (400 random regions vs a bitmap reference evaluator), T1 xhit port against an independent region and unprojection oracle (`8f44f8b`, corrective `1fa72af`); independent review `audit/phase-f-inc1-review/`
- **Pass criterion:** equals a reference region evaluator
- **Destination:** T0
- **May be closed by:** T0 property test against a reference region evaluator + T1 xhit port
- **Dependencies:** INC-1
- **Blocks:** INC-1 exit
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** Ratified CLOSED at INC-1 after independent review (CONDITIONAL PASS, findings INC1-F06/F07) and corrective verification (`1fa72af`, CI run 37291919770); record in evidence/phase-f/INC-1/corrective/.

### OB-3
- **Status:** CLOSED
- **Item:** Output removal during an active interaction (CR-3, X-11a). Test/decision: headless backend removes an output mid-pan/mid-move
- **Owner:** core interaction + output manager
- **Evidence available:** design C4.6
- **Pass criterion:** INV-1/INV-2 hold; pointer placed on a remaining output; no stuck buttons
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend) with real output removal (headless backend)
- **Dependencies:** INC-2, INC-5
- **Blocks:** INC-5 exit
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** 1R: constraint reconciliation rules (INV-INPUT-5) and 2R: removal via `output_disconnected` (INV-OUTPUT-1/2) are part of its criterion; see OB-31. Ratified CLOSED at INC-5 exit following independent adversarial review and owner ratification; verified by T1 production tests `removing_the_pan_output_orphans_the_pan` (`t1_input.rs`) and `ob31_xlock_grab_persistent_defer` (`t1_pointer_constraints.rs`).

### OB-4
- **Status:** CLOSED
- **Item:** Popup-grab dismissal on interaction acquisition (CR-4, X-11b). Test/decision: pan acquired while a popup grab is active
- **Owner:** core + adapter (W-SM-7 precondition)
- **Evidence available:** design C4.6, B6.3
- **Pass criterion:** `popup_done` in reverse creation order before the camera change; no panic
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend)
- **Dependencies:** INC-3, INC-4
- **Blocks:** INC-4 exit
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** INC-3 established the acquisition precondition ordering point. Adjudication of finding INC3-F08 confirms that D-B6-BOUNDARY rule 1 (anchor output selection from popup input event / grab serial) is owned by INC-4 alongside popup grab tracking and OB-4 dismissal. Ratified CLOSED at INC-4 exit following fifth independent review and ratification audit; verified by T1 production tests (`pf_ob4_interaction_acquisition_dismisses_popup_grab`, `r10_pan_with_popup_no_synthetic_events_and_clean_return`, and `a1_camerapan_overlapping_destruction`).

### OB-5
- **Status:** OPEN
- **Item:** X-7 games / network clients off-screen (DV-1). Test/decision: Wine/Proton game + a network client off-screen under withhold/kick/P
- **Owner:** core frame policy
- **Evidence available:** vkcube blocked; kick bounds it (EB-9)
- **Pass criterion:** no disconnect/freeze beyond P; CPU within budget
- **Destination:** T5
- **May be closed by:** T5 PHYSICAL campaign (separately authorized)
- **Dependencies:** INC-17
- **Blocks:** release / tuning only (not Phase E, not Phase F start)
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-6
- **Status:** OPEN
- **Item:** Fractional-camera snapping/rounding (CR-6). Test/decision: T0 properties: edge = round(L); re-projection lands in the same pixel; magnitudes to 2³⁶; random cameras
- **Owner:** core projection
- **Evidence available:** fractional cameras × 4 scales hit-test/render consistent (EC-20); X-6 edges
- **Pass criterion:** all hold
- **Destination:** T0
- **May be closed by:** T0 properties in PRODUCT core (edge = round(L), re-projection, 2³⁶ magnitudes, random cameras); physical-output part T4
- **Dependencies:** INC-1
- **Blocks:** INC-1 exit (T0 part); release (T4 part)
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** Physical-output part is T4 (C11.2).

### OB-7
- **Status:** OPEN
- **Item:** Downsampling / magnification quality (DV-2). Test/decision: visual comparison at m ∈ {0.67, 0.87, 1.04, 1.25, 2.67}
- **Owner:** backend renderer filters
- **Evidence available:** none (no renderer in harness)
- **Pass criterion:** subjective acceptance; text legibility
- **Destination:** T3
- **May be closed by:** T2 renderer-filter smoke + T3/T4 visual acceptance (PHYSICAL)
- **Dependencies:** INC-13, INC-15
- **Blocks:** release / tuning only
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-8
- **Status:** OPEN
- **Item:** Off-screen fallback period P (provisional 1 s). Test/decision: from OB-5 results
- **Owner:** configuration
- **Evidence available:** EB-9
- **Pass criterion:** as OB-5
- **Destination:** TUNE
- **May be closed by:** tuning decision after OB-5 (T5)
- **Dependencies:** OB-5
- **Blocks:** release / tuning only
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-9
- **Status:** OPEN
- **Item:** xwayland-satellite override-redirect menus/tooltips (C6-A1). Test/decision: X11 app with menus/tooltips (GTK3 `GDK_BACKEND=x11` under satellite in the harness; later Steam)
- **Owner:** satellite (external) + adapter popup path
- **Evidence available:** toplevel path verified (EC-12); menus untested
- **Pass criterion:** menus appear attached at the right place, receive input, dismiss correctly
- **Destination:** T3
- **May be closed by:** T1 GTK3/X11 menus via satellite (headless) then T3 Steam
- **Dependencies:** INC-12
- **Blocks:** INC-12 exit (T1 part); release (T3 part)
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-10
- **Status:** OPEN
- **Item:** RC-1 other toolkits. Test/decision: Qt6, SDL, Chromium/Electron, Firefox multi-window, Wine
- **Owner:** —
- **Evidence available:** GTK4 verified (EC-21)
- **Pass criterion:** no stale key/modifier; no client error
- **Destination:** T3
- **May be closed by:** T3 real-toolkit runs (PHYSICAL)
- **Dependencies:** INC-4, INC-15
- **Blocks:** release
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-11
- **Status:** OPEN
- **Item:** Smithay oneshot-lock defect W-SM-4b. Test/decision: file upstream issue; decide patch vs accept when pointer-lock code is implemented
- **Owner:** adapter / upstream
- **Evidence available:** reproduced (X-12)
- **Pass criterion:** X-12 reproducer passes (no `unlocked` for a never-active lock)
- **Destination:** UP
- **May be closed by:** upstream Smithay fix, or a Phase F patch decision under the D-B12-PIN patch policy, with X-12 passing
- **Dependencies:** —
- **Blocks:** INC-5 decision point (pointer constraints); release
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** E1R (unsealed, E-F2-02): scope extended from W-SM-4b to W-SM-4c (cross-batch duplicate `unlocked`/`unconfined` after an explicit deactivate; destruction; never-activated persistent). Candidate P1 (`if constraint.is_active()` in `leave()`) passes 11/11 sequences and X-12 (EE-2, EE-7). Patch-policy criteria: (1) reproduced — met; (2) no public-API fix — met; (3) upstream issue/PR — open (outward action for the user). The decision now gates the **whole** production constraint surface (INC-5), not only oneshot behaviour. Option B formally adjudicated and adopted by owner on October 7, 2026 under D-B12-PIN; satisfied and closed for the Phase F INC-5 release gate with patch P1 pinned in vendor/smithay-0.7.0; upstream issue submission tracked as outward user action.

### OB-12
- **Status:** OPEN
- **Item:** Scale constants H_cfg, r, σ (provisional). Test/decision: X-16 zoom sweep with real clients + visual
- **Owner:** configuration
- **Evidence available:** EC-16–19
- **Pass criterion:** renegotiations bounded per P3; CPU/memory acceptable; sharpness acceptable
- **Destination:** TUNE
- **May be closed by:** X-16 zoom sweep with real clients (T3/T4)
- **Dependencies:** INC-6, INC-15, INC-16
- **Blocks:** release / tuning only
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-13
- **Status:** OPEN
- **Item:** Device-limit enforcement and its supply through the renderer interface (the INC-6 part of the original OB-13). Core enforces the device capability it is given: h ≤ H_max for large windows, the integer hint bounded, the zero-extent domain. The backend obtains that capability from its renderer through the renderer capability interface at startup; a headless fixture may report a documented deterministic limit; no unexplained constant is passed directly into Core
- **Owner:** backend renderer → core ceiling
- **Evidence available:** EC-16
- **Pass criterion:** no buffer exceeds the reported device limit; the limit reaches Core only through the renderer capability interface; a change of the limit, the boundary values and an invalid limit are handled
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend)
- **Dependencies:** INC-6
- **Blocks:** INC-6 exit
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** E1R (unsealed, E-F2-06, E1R-N3): enforcement includes the zero-extent domain (`H_dev = +∞`, no division) and the integer emission `b(h)` (an integer-scale client's buffer must fit); model evidence EE-5. Residual: a logical side > `D_max` at scale 1. C12.F2 (owner amendment 2026-10-13, H-1): the device bound is on the 1/120 lattice, so the fractional hint itself no longer exceeds the device; the criterion is thereby achievable on the fixture for fractional-scale clients. Status unchanged: OPEN (a real renderer's limit is validated at INC-13). **Split 2026-10-14 (owner decision):** the original item, "Runtime `D_max` query and enforcement. Test/decision: query from the renderer at startup; assert h ≤ H_max for large windows", was listed by INC-6 and by INC-13 ("real `D_max`"). It is now two obligations whose union is the original: this one (enforcement, and the query through the renderer interface with a deterministic headless fixture) and OB-52 (the real renderer's limit, on a GPU). Nothing of the original criterion is dropped, and this one is not closed by the split. INC-6 × INC-7 integration (2026-10-15, not ratified): the INC-6 evidence holds on the integrated tree, but a protocol-fullscreen grant is a size Core chooses while F-SCALE-2's `s_max` counts only committed surfaces; with an observer of higher scale the hint in force can ask for a first fullscreen buffer over `D_max` (measured: 11,520 px for `D_max` 8192). Proposed layer C12.F11 (`evidence/phase-f/INC-6/C12F11_PROPOSAL.md`) counts the configured size; rehearsed on `rehearsal/inc-6-scale-amendments-2026-10-15` with tests and controls; with the hint now sent before the configure (grant, unset and output reconfiguration), the first buffer fits. Not met until C12.F11 (or another remedy) is adopted; status unchanged. C12.F11 adopted 2026-10-16 (owner decision, INC-6 adoption package D1): the configured fullscreen size and the restore offer count in `s_max(E)`. **Closure approved for the INC-6 exit (D7):** CLOSED with INC-6's ratification, not before; OB-52 (real renderer, T2) stays OPEN.

### OB-14
- **Status:** OPEN
- **Item:** Hit-test remaining coverage. Test/decision: transforms, zoom ≠ 1, sub-pixel clamp, multi-output spanning, random trees
- **Owner:** core
- **Evidence available:** see CP3A-2
- **Pass criterion:** 0 mismatches vs presented; clamp ≤ ½ px/k
- **Destination:** T0
- **May be closed by:** T0 properties + T1 production hit tests (transforms, zoom ≠ 1, clamp, spanning, random trees)
- **Dependencies:** INC-1, INC-10
- **Blocks:** INC-10 exit (zoom ≠ 1 part)
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** E1R (unsealed, E1R-N1, E-F3-02): non-Normal transforms are covered headlessly by EE-1 (harness, not production); F-HIT-1 corrected. The production transform part moves to OB-45.

### OB-15
- **Status:** CLOSED
- **Item:** Keyboard residual: compositor-side repeat (K9, unused) and output removal (K8 no-op). Test/decision: confirm no repeat events from the compositor; output removal changes no keyboard state
- **Owner:** core
- **Evidence available:** X-10
- **Pass criterion:** as stated
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend)
- **Dependencies:** INC-4
- **Blocks:** INC-4 exit
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** Ratified CLOSED at INC-4 exit following fifth independent review and ratification audit; verified by T1 production tests (`k8_ob15_output_removal_under_held_keys`, `k9_ob15_key_repeat_probe`, `t04_duplicate_presses`, `release_follows_press_owner_after_modifier_change`, and `sync_seat_key`).

### OB-16
- **Status:** OPEN
- **Item:** Input delivery as a kick trigger (CR-7). Test/decision: vkcube-class client off-screen + focused; keys delivered
- **Owner:** core frame policy
- **Evidence available:** design (B10.5)
- **Pass criterion:** first client response within one frame of the input
- **Destination:** T1
- **May be closed by:** T1 real-time harness port on the production path, then T3 toolkits
- **Dependencies:** INC-8
- **Blocks:** INC-8 exit (T1 part)
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-17
- **Status:** OPEN
- **Item:** `Window::geometry` re-clamp (W-SM-9). Test/decision: observe real CSD clients shrinking their tree
- **Owner:** adapter
- **Evidence available:** source
- **Pass criterion:** anchor stable; no jump
- **Destination:** T3
- **May be closed by:** T3 real CSD clients
- **Dependencies:** INC-1, INC-15
- **Blocks:** release
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-18
- **Status:** CLOSED
- **Item:** `grab_popup` panic precondition (W-SM-7) and `ensure_configured` (W-SM-6). Test/decision: misbehaving-client tests
- **Owner:** adapter
- **Evidence available:** source; INC-7 production: T1 `t1_popup_policy` (3 tests), `t1_inc7_misbehaving` (6 tests, with a bystander client); the protocol error code is Smithay's (W-SM-18)
- **Pass criterion:** protocol error to the client; compositor survives
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend) with misbehaving clients
- **Dependencies:** INC-4, INC-7
- **Blocks:** INC-7 exit
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** INC-4 satisfies the W-SM-7 popup grab panic precondition via `grab_popup` validation, `reconcile_popup_chain`, and adversarial tests (`a1_inner_popup_closes_then_key`, `a1_client_closes_whole_chain_then_key`, `a1_middle_popup_disappears_then_key`, `a1_outer_popup_disappears_then_key`, `a1_root_unmaps_then_key`, `a1_destruction_at_depth_4`). Per observation C-1, the W-SM-6 popup portion (`ensure_configured` / initial commit carrying buffer) remains open pending INC-7. OB-18 therefore remains OPEN blocking INC-7 exit. The W-SM-6 popup portion was closed by INC-7 slice 3 and the independent audit; ratified CLOSED at INC-7 exit (owner, 2026-10-14) on `99c24cd`.

### OB-19
- **Status:** CLOSED
- **Item:** Port the architecture/authority enforcement to the real crates (`verify-architecture`): dependency DAG across all dependency classes; camera-authority probes with asserted diagnostics (incl. 1R probes 2d/2e: no protocol re-fit, no retained proof; 2R probes 2f/2g: output-lifecycle token required, unforgeable, no coordinate fields, no public restore API, not storable in protocol state, single construction site; E1R probes 2h: reconfiguration probes p10–p13, output_reconfigured carries no camera authority; runtime 2i negative control probe_sealed_key_steal transferred to INC-2 / OB-49 via Decision D); `!Send` port/token + `Send` protocol-state assertion; source rules (`infynitae-wayland` never names `InputPort`, `ShellPort`, `OutputLifecycleToken`, `physical_input`, `shell_command`, `output_connected`, `output_reconfigured`, `output_disconnected`; `Authorized::mint` only in the entry module; authority reference/pointer escape rejected); `#![forbid(unsafe_code)]` in core and ipc, `#![deny(unsafe_code)]` elsewhere; `cargo build --locked`; `smithay = "=0.7.0"`; pinned `rust-toolchain.toml`.
- **Owner:** workspace tooling
- **Evidence available:** EC-1, EC-15, ER-4, ER2-1 (spike `verify.sh` PASS); INC-0 implementation (`632b1d0`), evidence commit (`0c229ad`), independent review dossier (`audit/phase-f-inc0-review/`), corrective patch and ratification
- **Pass criterion:** the same probes and checks run against the real crates in the first Phase F commit; every negative probe fails with its asserted diagnostic and no other error; every DAG/source check passes; CI-blocking; runtime 2i negative control executed in INC-2 under OB-49
- **Destination:** PHASE-F-ENTRY-GATE
- **May be closed by:** ENFORCE increment INC-0 only (the first Phase F product-workspace commit)
- **Dependencies:** —
- **Blocks:** Phase F: unconditional entry gate; no protocol or product behaviour may precede it. Not waivable. Satisfied and closed at INC-0.
- **Origin:** CP3A-4; OB-19 section; C12.R1.3; C12.R2.2; C12.E1R.4; Decision D
- **History:** Unconditional Phase F entry gate since Phase C; probe list extended in 1R (2d/2e), 2R (2f/2g), and E1R (2h). Ratified CLOSED in INC-0 following independent adversarial review and corrective patch (closing findings INC0-F01 and INC0-F02), with Decision D transferring runtime 2i validation to INC-2 / OB-49 while preserving INV-CAMERA-3 and INV-OUTPUT-2 without waiver. Not folded with OB-36.

### OB-20
- **Status:** OPEN
- **Item:** Games under KeepFocus pans and preserved locks (DV-3). Test/decision: real games
- **Owner:** core input
- **Evidence available:** X-8 (protocol level)
- **Pass criterion:** no camera jumps, no lost locks, no phantom motion
- **Destination:** T5
- **May be closed by:** T5 PHYSICAL (real games)
- **Dependencies:** INC-5, INC-17
- **Blocks:** release
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** Protocol-tier obligations (TST-05 x8 C3 pan with lock, C4 pan with confinement) verified at INC-5 exit by T1 production tests (`ob20_x8_c3_pan_with_lock`, `ob20_x8_c4_pan_with_confinement`). Physical hardware game testing remains assigned to INC-17 (T5 PHYSICAL).

### OB-21
- **Status:** OPEN
- **Item:** libinput device semantics under pan accumulation (DV-4). Test/decision: high-rate mice, touchpads (begin/end for gestures)
- **Owner:** backend
- **Evidence available:** none
- **Pass criterion:** camera = start − Q(Σ); no drift
- **Destination:** T4
- **May be closed by:** T4 PHYSICAL (libinput devices)
- **Dependencies:** INC-3, INC-16
- **Blocks:** release
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-22
- **Status:** OPEN
- **Item:** Fullscreen acceptance policy (focused-only / pre-map) UX. Test/decision: games, video players, X11 via satellite
- **Owner:** core protocol policy
- **Evidence available:** C5.2 design; satellite pre-map (EC-13)
- **Pass criterion:** no output seized while the user is elsewhere; games enter fullscreen
- **Destination:** T3
- **May be closed by:** T1 answer-policy part (with OB-32), then T3 apps/video and T5 games
- **Dependencies:** INC-7
- **Blocks:** release (T3/T5 part)
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** 1R: every request is answered (INV-PRES-1); acceptance UX remains the open part. T1 answer-policy part met at INC-7 exit (owner, 2026-10-14): unfocused requests are declined (`an_unfocused_client_is_declined_and_nothing_changes`); a pre-map grant cannot end a presentation before its element maps (C12.F6; `a_pre_map_grant_takes_the_output_only_when_its_window_maps`, `a_pre_map_grant_that_never_draws_ends_nothing`); focus is not moved to hidden windows (C12.F7). The T3 (apps, video) and T5 (games) parts remain; release blocker.

### OB-23
- **Status:** OPEN
- **Item:** ScaleInteractionEpoch settlement timeout (PROV-5) and discrete-interaction renegotiation rate with real clients and real input timing (wheel, keys, shell streams, discrete Stage moves).
- **Owner:** core scale policy (configuration)
- **Evidence available:** MODEL (EC-22, ER-5 R2, ER2-2); no real-client timing data
- **Pass criterion:** renegotiations per discrete interaction ≤ (rungs crossed + 1) with the chosen T_settle; T_settle exceeds the measured discrete-input repeat interval; no perceptible scale lag
- **Destination:** TUNE
- **May be closed by:** T3 real-client + T4 real-input measurements (PHYSICAL)
- **Dependencies:** INC-6, INC-15, INC-16
- **Blocks:** release / tuning only
- **Origin:** CP3A-4; rescoped C12.R1.3 / C12.R2.2
- **History:** Sealed scope was wheel-zoom oscillation; 1R: tunes T_settle; 2R: includes Stage-move interactions. PROVISIONAL.

### OB-24
- **Status:** OPEN
- **Item:** Zoom anti-jitter decision: the ±1 physical-pixel shape variation at lattice-reference switches while panning at zoom ≠ 1 (A-B1-1). **Not resolved and not validated away.** Render/hit agreement is intact (ED-1).
- **Owner:** architecture (core projection) via change control
- **Evidence available:** ED-1 (Smithay geometry shows the ±1 px change), ED-2 F1 (model: constant widths with a gesture-pinned reference, candidate only)
- **Pass criterion:** an architecture decision recorded through change control (a C12.R3-style amendment) with evidence equivalent to X-6/ED-1 Smithay geometry runs; the decided rule shows no relative shape change during zoomed pans, or the residual is explicitly accepted with visual evidence (T3/T4)
- **Destination:** DECISION
- **May be closed by:** architecture change control only (not an implementation-local choice)
- **Dependencies:** INC-9 (HARNESS evidence run)
- **Blocks:** INC-10: no PRODUCT increment may let persistent zoom ≠ 1 coexist with pan until resolved or the conservative fallback (fit-exit restores the baseline zoom) is active
- **Origin:** CP3A-4; rescoped C12.R1 (CORRECTIONS_1R)
- **History:** Sealed premise "static fit only" falsified (PD-F2-06); now a pre-implementation decision gate.

### OB-25
- **Status:** OPEN
- **Item:** Real presentation timing (DRM flips, VRR) beyond X-9. Test/decision: DRM backend presentation feedback
- **Owner:** backend + core clock
- **Evidence available:** X-9 (virtual)
- **Pass criterion:** `presented` timestamps from flips; clock output = sync_output
- **Destination:** T4
- **May be closed by:** T4 PHYSICAL (DRM flip timestamps, VRR)
- **Dependencies:** Phase G DRM backend
- **Blocks:** release
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-26
- **Status:** CLOSED
- **Item:** Regression guard: never use `Window::surface_under`; keep xsnap/xhit/X-15 as tests. Test/decision: port xsnap/xhit to T1
- **Owner:** core + adapter
- **Evidence available:** EC-9, EC-20; INC-1 production: T1 `tst09_xsnap_x15_and_surface_under_control`, `tst10_xhit_selection_equals_presented`, `tst04_x6…`, `tst18_d5hit_zoom1…` with their negative controls; source rule `smithay-rejected-apis` (`Space`, `surface_under`, `render_elements_from_surface_tree`, `output_enter/leave`) with seeded controls (`8f44f8b`, corrective `1fa72af`); independent review `audit/phase-f-inc1-review/`
- **Pass criterion:** all pass
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend) (xsnap/xhit/X-15 ported; `Window::surface_under` never used)
- **Dependencies:** INC-1
- **Blocks:** INC-1 exit
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** Ratified CLOSED at INC-1 after independent review (CONDITIONAL PASS, findings INC1-F06/F07) and corrective verification (`1fa72af`, CI run 37291919770); the regression guard stays enforced by `tools/verify-architecture` and the T1 tests; record in evidence/phase-f/INC-1/corrective/.

### OB-27
- **Status:** OPEN
- **Item:** Satellite: activation and primary selection behaviour. Test/decision: offer `xdg_activation_v1` (+ primary selection when added); X11 apps request focus
- **Owner:** adapter globals
- **Evidence available:** warnings observed (EC-12)
- **Pass criterion:** B10.7 policy applies; no camera move
- **Destination:** T3
- **May be closed by:** T3 toolkit/satellite activation runs
- **Dependencies:** INC-12
- **Blocks:** release
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-28
- **Status:** OPEN
- **Item:** Client resizing in response to scale hints, for the stimuli INC-6 provides (the INC-6 part of the original OB-28): an adversarial self-resizing client under SC-E after a non-client stimulus, under P7, and inside a continuous camera-pan epoch; cross-client presentation churn (C12.F13).
- **Owner:** core scale policy
- **Evidence available:** MODEL EC-22, ER-5 R2/R3, ER2-2
- **Pass criterion:** with an adversarial T1 client: hint changes ≤ (rungs crossed + 1 + one P7 per non-client stimulus) per epoch; the run ends quiescent; no camera mutation from client geometry (INV-CAMERA-1)
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend)
- **Dependencies:** INC-6
- **Blocks:** INC-6 exit
- **Origin:** CP3A-4; rescoped C12.R1.3
- **History:** 1R removed the re-fit loop (one-shot fit). INC-6 × INC-7 integration (2026-10-15, not ratified): T1 adversaries that fail without the rule they test: SC-E after a non-client stimulus (`adversarial_client_churn_bounds_sc_e`; the earlier version passed with SC-E disabled, H-8), P7 (`adversarial_client_under_p7_changes_the_hint_once`), a continuous camera-pan epoch (`adversarial_client_inside_a_pan_epoch`: one rung crossing, one settlement), each with a seeded control; no camera mutation from client geometry. Not coverable before later increments: the viewport-fit feedback loop of EC-22 (INV-CAMERA-5 has no code; INC-10, whose entry needs INC-6's exit) and discrete CameraZoom, WindowMove/Resize and ShellStream epochs (INC-10, INC-14, INC-11). Closing OB-28 at the INC-6 exit therefore needs an owner decision on that remainder (for example a split like OB-13 → OB-52); status unchanged. **Split 2026-10-16 (owner decision, INC-6 adoption package D5):** the union of OB-28 and OB-53 is the original; nothing of its criterion is dropped. **Closure approved for the INC-6 exit (D7):** the status changes to CLOSED with INC-6's ratification, not before.

### OB-29
- **Status:** CLOSED
- **Item:** Phase C 3B deliverables (C11–C16, final contradiction audit). Test/decision: checkpoint 3B
- **Owner:** —
- **Evidence available:** Phase C seal 615b0ed
- **Pass criterion:** C11–C16 and the final contradiction audit delivered (closed by the Phase C seal 615b0ed)
- **Destination:** GATE-C
- **May be closed by:** Phase C checkpoint 3B
- **Dependencies:** —
- **Blocks:** Phase C gate (historical)
- **Origin:** CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)

### OB-30
- **Status:** CLOSED
- **Item:** Port xpopup_focus (ER-1) to T1 against the production adapter: ActiveElementFocus vs ProtocolSurfaceRoute, ordered preemption (INV-INPUT-10), nested-grab W-SM-14 workaround.
- **Owner:** core FocusModel + adapter
- **Evidence available:** ER-1 (harness, T1-style)
- **Pass criterion:** PF-1…PF-7 pass on the production path; NEG-1 (naive set_focus), NEG-2 (pointer-first) and OBS-1 (nested grab replacing the pointer grab) fail as predicted; no panic, no duplicate enter/leave, no stuck key
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend)
- **Dependencies:** INC-4
- **Blocks:** INC-4 exit
- **Origin:** CORRECTIONS_1R (1R)
- **History:** Ratified CLOSED at INC-4 exit following fifth independent review and ratification audit; verified by T1 production tests (`pf_1_active_element_vs_protocol_surface_route` through `pf_7_protocol_error_on_unmapped_grab`, `ordered_preemption_exact_keyboard_wire`) and permanently gated by `./tools/verify-architecture` check 5d verifying negative controls NEG-1, NEG-2, and OBS-1 under probe flags.

### OB-31
- **Status:** CLOSED
- **Item:** Port xlock_grab (ER-2) to T1 with real headless output removal; add confinement cases (ER-5 R5).
- **Owner:** core PointerModel + adapter
- **Evidence available:** ER-2 (harness), ER-5 R5 (model)
- **Pass criterion:** DEFER passes for persistent and oneshot locks (core = Smithay at every step, exactly one `unlocked`, no motion while locked); SEALED and EXPLICIT/persistent controls fail; confinement never leaves the region and never crosses outputs
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend)
- **Dependencies:** INC-5
- **Blocks:** INC-5 exit
- **Origin:** CORRECTIONS_1R (1R)
- **History:** Ratified CLOSED at INC-5 exit following independent adversarial review and owner ratification; verified by T1 production tests (`ob31_xlock_grab_persistent_defer`, `ob31_xlock_grab_oneshot_defer`) and core unit tests (`pointer.rs::m14_confinement_clamping_and_arrangement_isolation`, `m13_reconciliation_deferred_under_client_grab`).

### OB-32
- **Status:** CLOSED
- **Item:** Port xmaximize (ER-3) to T1 and test the `activated` mapping (INV-PROTO-1, INV-PRES-1).
- **Owner:** core protocol policy + adapter
- **Evidence available:** ER-3 (harness); INC-7 production: T1 `t1_toplevel_requests` (6 tests), `t1_fullscreen` (9 tests), the `t1_inc7_adversarial` request streams; record-only and deferral seeded controls detected (`tools/verify-test-efficacy`)
- **Pass criterion:** maximize/unmaximize and set/unset_fullscreen each answered by exactly one configure with the policy state; record-only/deferral controls fail; `activated` set exactly on ActiveElementFocus; anchors and cameras unchanged
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend)
- **Dependencies:** INC-7
- **Blocks:** INC-7 exit
- **Origin:** CORRECTIONS_1R (1R)
- **History:** Ratified CLOSED at INC-7 exit (owner, 2026-10-14) on `99c24cd`.

### OB-33
- **Status:** OPEN
- **Item:** xwayland-satellite restart on the same X display via compositor-owned sockets and `-listenfd` (PD-F2-16).
- **Owner:** bin supervisor
- **Evidence available:** ER-6 (`--test-listenfd-support` exit 0); EC-12 (crash path)
- **Pass criterion:** kill satellite while an X11 client runs; restart serves the same `DISPLAY`; a new X11 client launched with the old `DISPLAY` connects; elements of dead X11 clients are destroyed through client death
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend) with real xwayland-satellite (HEADLESS)
- **Dependencies:** INC-12
- **Blocks:** INC-12 exit
- **Origin:** CORRECTIONS_1R (1R)

### OB-34
- **Status:** OPEN
- **Item:** Cross-output drag decision (PD-F2-13, F-DRAG-1): Canvas delta definition when the pointer crosses between independently-camera'd outputs. **Open design decision.**
- **Owner:** architecture via change control
- **Evidence available:** analysis only (PD-F2-13)
- **Pass criterion:** a recorded architecture decision; then T0 property (batching-independent, no drift) and T1 test (element stays under the cursor across an Arrangement crossing, or the decided alternative)
- **Destination:** DECISION
- **May be closed by:** architecture change control only
- **Dependencies:** —
- **Blocks:** INC-14 (interactive window move/resize) may not start before it is decided
- **Origin:** CORRECTIONS_1R (1R)

### OB-35
- **Status:** OPEN
- **Item:** Attention / `transient_for` lifecycle on unmap and destroy (PD-F2-15).
- **Owner:** core FocusModel / Stage metadata
- **Evidence available:** analysis only
- **Pass criterion:** T1 shell-event test: no attention mark survives unmap or destroy (with a shell event); no stale `transient_for` after the parent's unmap/destroy
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend)
- **Dependencies:** INC-11
- **Blocks:** INC-11 exit
- **Origin:** CORRECTIONS_1R (1R)

### OB-36
- **Status:** CLOSED
- **Item:** Run `tools/check-foundation-docs` (positive run and its seeded negative controls) in CI beginning with the first Phase F commit.
- **Owner:** workspace tooling
- **Evidence available:** TST-25 (checker PASS; negative controls fail as intended, D7); GitHub Actions CI workflow (`.github/workflows/ci.yml`) and passing CI runs on main
- **Pass criterion:** a CI job executes `tools/check-foundation-docs` and `tools/check-foundation-docs --self-test` on every commit; any failure blocks merge
- **Destination:** CI
- **May be closed by:** ENFORCE increment INC-0 (separate check from OB-19)
- **Dependencies:** —
- **Blocks:** Phase F: required in the first Phase F commit alongside OB-19, but not part of OB-19
- **Origin:** D7 (new)
- **History:** Closed at INC-0 with CI automation established in `.github/workflows/ci.yml` and verified passing on remote main.

### OB-37
- **Status:** OPEN
- **Item:** Pan cursor behaviour (PROV-7): frozen cursor vs cursor-follows-world
- **Owner:** product policy (Level C/D)
- **Evidence available:** Phase B ledger category 3 (provisional); no measurement
- **Pass criterion:** a recorded decision after T3 UX evaluation and T5 game observation; changing the policy goes through an invariant amendment (INV-INPUT-3, INV-COORD-5)
- **Destination:** DECISION
- **May be closed by:** recorded decision / PHYSICAL evidence
- **Dependencies:** —
- **Blocks:** release / feature (Level D) only
- **Origin:** D7 (PD-F1-10)

### OB-38
- **Status:** OPEN
- **Item:** Popup draw order (PROV-8): popups immediately above their own element
- **Owner:** product policy (Level C/D)
- **Evidence available:** Phase B ledger category 3 (provisional); no measurement
- **Pass criterion:** a recorded Level D decision; any change amends INV-RENDER-2 / INV-HIT-1
- **Destination:** DECISION
- **May be closed by:** recorded decision / PHYSICAL evidence
- **Dependencies:** —
- **Blocks:** release / feature (Level D) only
- **Origin:** D7 (PD-F1-10)

### OB-39
- **Status:** OPEN
- **Item:** Integer preferred scale ⌈h⌉ (PROV-9)
- **Owner:** product policy (Level C/D)
- **Evidence available:** Phase B ledger category 3 (provisional); no measurement
- **Pass criterion:** T3 integer-only clients show acceptable sharpness/memory at ⌈h⌉; any change amends INV-SCALE-5
- **Destination:** T3
- **May be closed by:** recorded decision / PHYSICAL evidence
- **Dependencies:** —
- **Blocks:** release / feature (Level D) only
- **Origin:** D7 (PD-F1-10)

### OB-40
- **Status:** OPEN
- **Item:** Activation outcomes and shell UX (PROV-10)
- **Owner:** product policy (Level C/D)
- **Evidence available:** Phase B ledger category 3 (provisional); no measurement
- **Pass criterion:** a recorded shell-design decision (Level D) consistent with INV-INPUT-9; toolkit behaviour from OB-27
- **Destination:** DECISION
- **May be closed by:** recorded decision / PHYSICAL evidence
- **Dependencies:** —
- **Blocks:** release / feature (Level D) only
- **Origin:** D7 (PD-F1-10)

### OB-41
- **Status:** OPEN
- **Item:** Reserved suspension reason StableUnobserved(T) (PROV-11)
- **Owner:** product policy (Level C/D)
- **Evidence available:** Phase B ledger category 3 (provisional); no measurement
- **Pass criterion:** enabled only with T3/T5 evidence of benefit, through an INV-FRAME-6 amendment; otherwise stays disabled
- **Destination:** DECISION
- **May be closed by:** recorded decision / PHYSICAL evidence
- **Dependencies:** —
- **Blocks:** release / feature (Level D) only
- **Origin:** D7 (PD-F1-10)

### OB-42
- **Status:** OPEN
- **Item:** Hot-path allocation contract (INV-ALLOC-1): counting-allocator test around a scripted steady-state pan/route/key/hit-test/visibility/render-list sequence after warm-up (C14.3).
- **Owner:** core
- **Evidence available:** design only (C14.3)
- **Pass criterion:** zero heap allocations per event after warm-up, every round, at 10, 100 and 1,000 elements, on each Core hot path INV-ALLOC-1 lists (input intake, pan, pointer routing, hit-testing, visibility, render-list construction) and on frame tick and commit (C14.3); counted by a test-only counting global allocator, per thread, around calls of Core's production caller-buffer entries only; frees are not counted
- **Destination:** T0
- **May be closed by:** T0 PRODUCT test of Core's entries. Its test binary is outside Core (INV-ARCH-2 forbids `unsafe` in every Core target; owner direction 2026-10-15): today `crates/infynitae-backend-headless/tests/t0_hot_path_allocations.rs`. Conditions, each checked: the file names no crate but `std` and `infynitae_core` (`verify-architecture`, with two seeded violations); production reaches only the measured caller-buffer entries (source rule `callers-use-caller-buffer-entries`, four seeded violations); seeded reintroductions of an allocation are detected (`verify-test-efficacy`)
- **Dependencies:** INC-3, INC-6
- **Blocks:** INC-6 exit (all listed hot paths exist by then)
- **Origin:** D7 (PD-F1-11)
- **History:** Core visibility remediation (`85a74cb`, 2026-10-13): allocations per call counted outside the repository with a counting allocator in a copy of `core_bench` at 3,000 elements — pan event 9,005 → 10, render list 9,004 → 10, commit 9,006 → 10, pointer motion 1, frame tick 3. The criterion (zero) is not met and no T0 counting-allocator test exists; status unchanged. INC-6 P-1 repair (`perf/inc-6-scale-index`, 2026-10-14, experimental): a T0 counting-allocator **ratchet** exists (`crates/infynitae-core/tests/t0_hot_path_allocations.rs`, owner-approved test-class unsafe, U-002…U-004): pan 15, route 1, key 2, commit 9, render list 15, frame tick 4 per event, equal to `main`, independent of the number of windows. It is not the zero-allocation criterion; status unchanged. INC-6 × INC-7 integration (`integration/inc-6-exit-hardening-2026-10-15`, 2026-10-15, not ratified): the counts above are those of `ccb82c5`. With ratified INC-7, commit is 10 and off-view commit 5 per event: one `Vec` per root commit from INC-7's presentation snapshot (`Core::presentations`, `f98309b`), measured equal on `main` `23752f1`; INC-6 adds no allocation on the measured paths (attribution by backtrace in a scratch copy of the test). A ninth measured path, a commit of a window presented fullscreen, allocates 24 per commit whatever the number of windows, equal on `main` `23752f1`. Every remaining allocation is Core's own (temporary `Vec`s of surface origins and visibility, membership rows, frame entries, the returned `Vec<Effect>`); zero needs C14.3's caller-owned effect buffers (18 public Core entries, 42 functions return `Vec<Effect>`) and reused per-output buffers. The criterion (zero) is not met; status unchanged. INC-6 final-resolution branch (`integration/inc-6-final-resolution-2026-10-15`, 2026-10-15, not ratified): **zero allocations per event after warm-up on every measured path** (pan, crossing pan, route, key, commit, off-view commit, render list, frame tick, commit of a fullscreen window), at 10, 100 and 1,000 windows, every round; the test asserts 0 and moved to `crates/infynitae-backend-headless/tests/t0_hot_path_allocations.rs` (owner direction; INV-ARCH-2 unchanged). Design per C14.3: `physical_input` takes the caller's `EffectBuf`, `toplevel_committed_into` and `frame_tick_into` append to caller storage, per-element tree layouts, reused membership/frame/scratch storage, shared input regions; six seeded reintroductions of an allocation are detected. Scope: the measured steady state (one scale-1 output); a batch that emits a scale hint or settles an epoch, a constraint change, a key binding's action and a button press still return a `Vec` (C14.3 'steady state'; listed in `evidence/phase-f/INC-6/FINAL_RESOLUTION_2026-10-15.md`). The criterion is met on that branch for the measured paths; closing OB-42 is the owner's act at the INC-6 exit review; status unchanged. **Wording corrected 2026-10-16 (owner decision, INC-6 adoption package D6):** pass criterion and closure condition restated for a test outside Core; the evidence required is unchanged and the Core-only condition is checked mechanically. Measured on `eebb5c3`: 0 allocations per event on nine paths at 10, 100 and 1,000 windows; scope: steady state only (a batch that emits a scale hint or settles an epoch, a constraint change, a binding's action and a button press still return a `Vec`, FINAL_RESOLUTION_2026-10-15.md). **Closure approved for the INC-6 exit (D7):** CLOSED with INC-6's ratification, not before.

### OB-43
- **Status:** OPEN
- **Item:** Persistence load (INV-FAIL-1, INV-OUTPUT-2): validated all-or-nothing load; corrupt file never partially applied; cameras only via `output_connected`; anchors only via placement.
- **Owner:** core persistence service
- **Evidence available:** design only (C8, M20)
- **Pass criterion:** T1: a corrupt or partially valid file yields an empty/default state and is kept for inspection; a valid file restores cameras only through the token path
- **Destination:** T1
- **May be closed by:** T1 production test (PRODUCT increment, production core + adapter + headless backend) when persistence (Level D format) is implemented
- **Dependencies:** Level D persistence format
- **Blocks:** the persistence feature (Level D); not Phase E or Phase F start
- **Origin:** D7 (PD-F1-11)

### OB-44
- **Status:** OPEN
- **Item:** Performance and idle baselines (C14.1/C14.2): input latency, hit-test, projection, render-element generation, GPU submission, IPC, input-to-presentation latency, memory, product-idle wakeups (INV-IDLE-1).
- **Owner:** workspace / backend
- **Evidence available:** none (no production code; harness numbers are not baselines, C14.1)
- **Pass criterion:** C14 procedures executed on target hardware; budgets frozen only from these T4 baselines; zero Infynitae-attributable wakeups while product-idle
- **Destination:** T4
- **May be closed by:** T4 PHYSICAL campaign (EXCLUSIVE_HARDWARE, separately authorized)
- **Dependencies:** INC-16
- **Blocks:** release
- **Origin:** D7 (PD-F1-11)
- **History:** Core visibility remediation (`85a74cb`, 2026-10-13): T0 model timings of Core entry points are recorded in `evidence/phase-f/core-visibility/INTEGRATION_RECORD.md`. They are not C14 baselines (no hardware, no renderer); status unchanged.

### OB-45
- **Status:** CLOSED
- **Item:** Non-Normal output transforms on the production path (E-F2-01, E1R-N1, E-F3-02): damage tracker sized `phys_O`, view `view_O`, render placement, damage, membership, hit-test, project/unproject round trip and boundary rounding for Normal/90/180/270 (+ flipped), asymmetric modes, fractional scale with rotation.
- **Owner:** core + adapter
- **Evidence available:** EE-1 (headless harness with a GLES-semantics recording renderer; not production, not a GPU); INC-2 production damage tracker (`phys_O`); INC-3 production hit-testing (`ee1_c5_render_hit_agreement_on_the_production_path` across 10 transform cases, 0 mismatches, sealed hit control fails 9/9 non-Normal cases).
- **Pass criterion:** the EE-1 matrix reproduced on the production adapter with the headless backend: 0 failing checks and 0 render/hit mismatches; the sealed tracker size and the sealed hit rule fail as negative controls
- **Destination:** T1
- **May be closed by:** T1 production tests (INC-2: tracker, membership, damage; INC-3: hit-test)
- **Dependencies:** INC-2, INC-3
- **Blocks:** INC-3 exit. Satisfied and closed at INC-3.
- **Origin:** E1R (E-F2-01, E1R-N1, E-F3-02)
- **History:** E1R obligation for rotated output transforms. INC-2 satisfied tracker, membership, and damage. INC-3 satisfied physical hit-testing agreement and project/unproject round trips across all 10 transform cases under standard ulimit -n 1024. Ratified CLOSED at INC-3 exit following resolution of INC3-F09.

### OB-46
- **Status:** OPEN
- **Item:** Rotated outputs on real hardware: DRM/KMS mode and transform, GLES viewport, cursor plane, handedness of 90°/270° on glass, damage on a rotated panel.
- **Owner:** backend (Phase G DRM)
- **Evidence available:** none physical (EE-1 is headless; INC-13 adds a NESTED GLES check)
- **Pass criterion:** a 1920×1080 panel at 90°/270° and fractional scale shows no clipping, gap or extra pixels; hit-test agrees with what is shown
- **Destination:** T4
- **May be closed by:** T4 PHYSICAL campaign (EXCLUSIVE_HARDWARE, separately authorized)
- **Dependencies:** INC-16
- **Blocks:** release claims for rotated outputs
- **Origin:** E1R (E-F2-01)

### OB-47
- **Status:** CLOSED
- **Item:** Production constraint-lifecycle regression (E-F2-02): the EE-2 sequences E1–E11 (explicit deactivate then later exit, destruction, output removal, reactivation, held button, confinement, never-activated persistent and oneshot, leave-instead alternative) and the EE-3 constraint cases on the production adapter.
- **Owner:** adapter + core
- **Evidence available:** EE-2, EE-3 (harness), EE-7
- **Pass criterion:** every sequence valid (activation only from inactive, deactivation only from active, nothing after a oneshot end) with the dependency graph actually pinned; the published-Smithay build shows W-SM-4c as the negative control until OB-11 is decided
- **Destination:** T1
- **May be closed by:** T1 production tests in INC-5 after the OB-11 decision is applied to the pinned graph
- **Dependencies:** OB-11, INC-5
- **Blocks:** INC-5 exit
- **Origin:** E1R (E-F2-02)
- **History:** Ratified CLOSED at INC-5 exit following independent adversarial review and owner ratification; verified by T1 production tests (`ob47_e1_persistent_explicit_deactivate_then_exit`, `ob47_e3_oneshot_explicit_deactivate_defunct`, `round5_pointer_constraints_stress_200_cycles`); zero duplicate deactivations under authorized patch P1.

### OB-48
- **Status:** OPEN
- **Item:** Output persistence identity (E-F3-03): how `OutputKey` behaves on real platforms — missing or zero EDID serial, two identical monitors, a physically different monitor on the same connector, dock/MST topology changes, connector reuse, virtual/headless outputs — and whether cameras for keys known to be ambiguous are persisted at all.
- **Owner:** backend + core persistence
- **Evidence available:** EE-4 (spike: deterministic collision rule); no platform observation
- **Pass criterion:** documented key derivation per backend; observed behaviour on the listed topologies matches INV-CAMERA-3's collision rule; an architecture decision on persisting ambiguous keys
- **Destination:** DECISION
- **May be closed by:** architecture change control with T4 observations
- **Dependencies:** INC-16 (observations)
- **Blocks:** gate G-OB48 (cross-session persisted camera state as a product claim); in-session behaviour is not blocked
- **Origin:** E1R (E-F3-03)

### OB-49
- **Status:** CLOSED
- **Item:** Output reconfiguration on the production path (E-F2-03): `output_reconfigured` for mode, transform, scale and refresh changes; same `OutputId`; camera identity and value unchanged; tracker/view/membership rebuilt; F-RECONF-1; `wl_output`/`xdg_output` events; rejection of unknown ids and key changes; reconnect tested separately; ProjectionFrame dropped on reconfiguration (closing INC2-F07). Owns runtime 2i validation (transferred from OB-19 via Decision D): runtime 1:1 camera/output binding preservation and `OutputKey` collision negative control (`probe_sealed_key_steal` / `concurrent_output_key_collision`), proving that `INV-CAMERA-3` and `INV-OUTPUT-2` hold during multi-output binding. Decision E ownership correction: protocol fullscreen configure is transferred to INC-7 (INV-PRES-1, OB-22/OB-32); ScaleInteractionEpoch settlement is transferred to INC-6 (ScaleInteractionEpoch, OB-13/OB-47); neither blocks INC-2 exit.
- **Owner:** core output manager + adapter
- **Evidence available:** EE-4 (spike tests and probes p10–p13, 2h; spike 2i `probe_sealed_key_steal`); INC-2 implementation (`0a03aab`), review dossier (`audit/phase-f-inc2-review/`), and corrective verification
- **Pass criterion:** the EE-4 reconfiguration tests ported to the production core and adapter; probes p10–p13 hold in the production crates; reconnect and reconfiguration remain separate test cases; runtime 2i negative control (`probe_sealed_key_steal`) executes against the real output manager and fails the buggy/stealing binding model while passing the 1:1 model; `reconfiguration_drops_the_output_frame` verifies ProjectionFrame invalidation on reconfiguration (closing finding INC2-F07); Decision E ratifies transfer of fullscreen configure to INC-7 and epoch settlement to INC-6.
- **Destination:** T1
- **May be closed by:** T1 production tests (INC-2)
- **Dependencies:** INC-2
- **Blocks:** INC-2 exit. Satisfied and closed at INC-2.
- **Origin:** E1R (E-F2-03); Decision D (OB-19 2i runtime validation transfer); Decision E (ownership correction transfer of fullscreen configure to INC-7 and epoch settlement to INC-6)
- **History:** E1R obligation for output reconfiguration. Decision D added runtime 2i validation. Ratified CLOSED in INC-2 following independent adversarial review and corrective patch (closing finding INC2-F07), with Decision E transferring protocol fullscreen configure to INC-7 and ScaleInteractionEpoch settlement to INC-6 while preserving INV-CAMERA-3 and INV-OUTPUT-2 without waiver.

### OB-50
- **Status:** CLOSED
- **Item:** Zero-output lifecycle on the production path (E-F2-05): one output → focused interaction → last output removed → `NoOutput` → reconnect, with no constraint, persistent lock, oneshot lock, confinement and a client-held button; exact wire order of enter/leave/locked/unlocked.
- **Owner:** core + adapter
- **Evidence available:** EE-3 (harness, stock and P1), EE-4 (spike)
- **Pass criterion:** the EE-3 cases valid on the production adapter (leave at removal, one constraint end, no event in the zero-output state, enter after reconnect, no absolute motion to a locked surface); N1–N3 negative controls fail
- **Destination:** T1
- **May be closed by:** T1 production tests (core part INC-2; pointer part INC-3; constraint part INC-5)
- **Dependencies:** INC-2, INC-3, INC-5
- **Blocks:** INC-5 exit (and the multi-output lifecycle is not complete before it)
- **Origin:** E1R (E-F2-05)
- **History:** Ratified CLOSED at INC-5 exit following independent adversarial review and owner ratification; verified across persistent lock, oneshot lock, confinement, and held button by T1 production tests (`ob50_zero_output_persistent_lock`, `ob50_zero_output_oneshot_lock`, `ob50_zero_output_confinement`, `ob50_zero_output_with_held_button`).

### OB-51
- **Status:** CLOSED
- **Item:** E1R-N2: INV-INPUT-10 step 3 (pointer `unset_grab`) may deliver one absolute motion at an unchanged location to a surface holding an active lock (W-SM-16). Test the popup-grab + active-lock + focus-change sequence; if reproduced, replace step 3 by the zero-output-style `set_grab(…, Focus::Clear)` ordering through change control.
- **Owner:** adapter
- **Evidence available:** EE-3 N3 (the mechanism, in the zero-output configuration)
- **Pass criterion:** no wl_pointer.motion reaches a locked surface during a focus preemption
- **Destination:** T1
- **May be closed by:** T1 production test in INC-5 (needs INC-4 popup routing)
- **Dependencies:** INC-4, INC-5
- **Blocks:** INC-5 exit
- **Origin:** E1R (E1R-N2)
- **History:** Ratified CLOSED at INC-5 exit following independent adversarial review and owner ratification; verified by T1 production test `ob51_popup_preemption_under_lock` (zero absolute motion to locked root surface W during popup grab; lock survives popup teardown).

### OB-52
- **Status:** OPEN
- **Item:** Real renderer device limit (the INC-13 part of the original OB-13). The limit is queried from the real renderer and device at startup and again after renderer recreation, propagated to Core through the same renderer capability interface, and the enforcement of OB-13 is shown against that value on a real GPU. No fixture result substitutes for hardware evidence
- **Owner:** backend renderer (GLES) → core ceiling
- **Evidence available:** none: the nested backend reports the provisional EC-16 value without querying its renderer, and the windowless GLES test queries no limit
- **Pass criterion:** the value Core holds equals the limit the GPU driver reports; no buffer exceeds it in a nested run; after renderer recreation the value is re-queried
- **Destination:** T2
- **May be closed by:** T2 nested run on a real GPU in INC-13 (separately authorized); not by any headless or fixture test
- **Dependencies:** INC-13, OB-13
- **Blocks:** INC-13 exit
- **Origin:** OB-13 split (owner decision 2026-10-14); the requirement itself: CP3A-4 (ARCHITECTURE, 795800937a / sealed 615b0ed)
- **History:** Created by the split of OB-13. Until then INC-13 cited "OB-13 (real `D_max`)".

### OB-53
- **Status:** OPEN
- **Item:** Client resizing in response to scale hints, for the stimuli later increments introduce (the remainder of the original OB-28): the viewport-fit feedback loop of EC-22 (INV-CAMERA-5) and discrete CameraZoom epochs (INC-10), WindowMove / WindowResize epochs (INC-14), ShellStream epochs (INC-11).
- **Owner:** core scale policy + the owning increment
- **Evidence available:** MODEL EC-22, ER-5 R2/R3, ER2-2
- **Pass criterion:** with an adversarial T1 client: hint changes ≤ (rungs crossed + 1 + one P7 per non-client stimulus) per epoch; the run ends quiescent; no camera mutation from client geometry (INV-CAMERA-1); re-fit rate ≤ commit rate (EC-22)
- **Destination:** T1
- **May be closed by:** T1 production tests in INC-10, INC-14 and INC-11, each for its stimuli; closes when all three parts are met
- **Dependencies:** INC-10, INC-11, INC-14, OB-28
- **Blocks:** the exits of INC-10 (fit loop, CameraZoom), INC-14 (WindowMove/Resize), INC-11 (ShellStream), each for its part
- **Origin:** OB-28 split (owner decision 2026-10-16, INC-6 adoption package D5); the requirement: CP3A-4; C12.R1.3
- **History:** Created by the split of OB-28.

<!-- /living -->

## 3. Test inventory (canonical `TST-*`)

Existing Phase B–D tests are **HARNESS**, **MODEL** or **ENFORCE-spike** category. None of them exercises the production adapter. Their parity destination is the PRODUCT increment that must port them as an exit criterion (FOUNDATION_IMPLEMENTATION_PLAN.md, "Harness parity").

<!-- living:TST -->
### TST-01 — b1-numeric (E1–E9)
- **Category / tier:** MODEL (offline numeric) / T0-class
- **Evidence:** EB-1
- **Negative control:** f64, f32, i32 and per-event quantisation variants
- **Production parity:** INC-1 (T0 properties in core)
- **Obligations:** OB-6

### TST-02 — b2-space (S1–S7)
- **Category / tier:** HARNESS (Smithay `Space`) / T1-style
- **Evidence:** EB-2
- **Negative control:** —
- **Production parity:** none (`Space` is not used; guarded by INC-1's "no `Space`" source check)
- **Obligations:** OB-26

### TST-03 — b8 scenarios SC1–SC4, X5
- **Category / tier:** HARNESS / T1-style lockstep
- **Evidence:** EB-8, EB-H1
- **Negative control:** Max-instant / Min / Primary / LargestArea scale policies (SC2); `Window::output_enter` popup membership (X5)
- **Production parity:** INC-2 (membership, clock), INC-6 (scale)
- **Obligations:** OB-1, OB-13

### TST-04 — x6 render edges
- **Category / tier:** HARNESS (DummyRenderer) / T1-style
- **Evidence:** EB-14
- **Negative control:** i32 root location (72,242 mismatches)
- **Production parity:** INC-1
- **Obligations:** OB-6, OB-26

### TST-05 — x8 pan input ownership (9 cases, 48 checks)
- **Category / tier:** HARNESS / T1-style
- **Evidence:** EB-13
- **Negative control:** explicit deactivate before a focus change (duplicate `unlocked`); INV-INPUT-3 bypass switch (to add in the port)
- **Production parity:** INC-3, INC-5
- **Obligations:** OB-4, OB-21

### TST-06 — x9 presentation feedback
- **Category / tier:** HARNESS / T1-style
- **Evidence:** EB-15
- **Negative control:** —
- **Production parity:** INC-2, INC-8
- **Obligations:** OB-25 (T4 part separate)

### TST-07 — x10 keyboard ownership (9 scenarios + NEG)
- **Category / tier:** HARNESS / T1-style
- **Evidence:** EC-6
- **Negative control:** K3 disabled → stuck modifier
- **Production parity:** INC-4
- **Obligations:** OB-15

### TST-08 — x12 oneshot-lock defect reproducer
- **Category / tier:** HARNESS / T1-style
- **Evidence:** X-12 (EB-16 context)
- **Negative control:** it *is* the defect reproducer (expected to show W-SM-4b until fixed)
- **Production parity:** INC-5 decision point
- **Obligations:** OB-11

### TST-09 — xsnap + X-15 order
- **Category / tier:** HARNESS / T1-style
- **Evidence:** EC-8, EC-9
- **Negative control:** `Window::surface_under` (W-SM-11/12 disagreements)
- **Production parity:** INC-1
- **Obligations:** OB-26, OB-1

### TST-10 — xhit physical-pixel hit test
- **Category / tier:** HARNESS / T1-style
- **Evidence:** EC-20
- **Negative control:** canvas-exact (D-B2-I9) rule, 399–617 mismatches
- **Production parity:** INC-1
- **Obligations:** OB-2, OB-14, OB-26

### TST-11 — realclient (vkcube, kitty, Firefox; Starve/Throttle/kick)
- **Category / tier:** HARNESS with real clients / T3-style, real time
- **Evidence:** EB-9
- **Negative control:** pure Starve (vkcube ping blocked 2–4 s)
- **Production parity:** INC-8 (T1 part), INC-15
- **Obligations:** OB-5, OB-16

### TST-12 — scaleprobe
- **Category / tier:** HARNESS with real clients / T3-style
- **Evidence:** EC-17
- **Negative control:** per-frame scale storm
- **Production parity:** INC-15
- **Obligations:** OB-12

### TST-13 — rc1 (GTK4 RC-1)
- **Category / tier:** HARNESS with real client / T3-style
- **Evidence:** EC-21
- **Negative control:** —
- **Production parity:** INC-15
- **Obligations:** OB-10

### TST-14 — c6-sat (xwayland-satellite)
- **Category / tier:** HARNESS with real satellite / T3-style
- **Evidence:** EC-12, EC-13
- **Negative control:** Smithay default fullscreen handler (W-SM-13)
- **Production parity:** INC-12
- **Obligations:** OB-9, OB-27, OB-33

### TST-15 — authority spike `verify.sh` (probes 1–2g, 8 unit tests)
- **Category / tier:** ENFORCE (spike) / T0
- **Evidence:** EC-1, EC-15, ER-4, ER2-1
- **Negative control:** every probe is a negative control (asserted diagnostic, exactly one error each); 2g seeded `reissue_token()`
- **Production parity:** INC-0
- **Obligations:** OB-19

### TST-16 — cp3a scale model (walkthroughs A–E)
- **Category / tier:** MODEL / T0-class
- **Evidence:** EC-19
- **Negative control:** raw ρ without ladder or ceiling (313 renegotiations)
- **Production parity:** INC-6
- **Obligations:** OB-12

### TST-17 — feedback model
- **Category / tier:** MODEL / T0-class
- **Evidence:** EC-22
- **Negative control:** naive settle-every-time
- **Production parity:** INC-6
- **Obligations:** OB-28

### TST-18 — d5hit
- **Category / tier:** HARNESS / T1-style
- **Evidence:** ED-1
- **Negative control:** —
- **Production parity:** INC-1 (zoom-1 configurations), INC-10 (zoomed configurations)
- **Obligations:** OB-14, OB-24 (residual observation)

### TST-19 — d5_model (D5 epochs, F1–F8)
- **Category / tier:** MODEL / T0-class
- **Evidence:** ED-2
- **Negative control:** sealed pan formula; canvas-exact rule
- **Production parity:** INC-10
- **Obligations:** OB-24

### TST-20 — xpopup_focus
- **Category / tier:** HARNESS / T1-style
- **Evidence:** ER-1
- **Negative control:** NEG-1 naive `set_focus`; NEG-2 pointer-first teardown; OBS-1 nested grab replacing the pointer grab
- **Production parity:** INC-4
- **Obligations:** OB-30

### TST-21 — xlock_grab
- **Category / tier:** HARNESS / T1-style
- **Evidence:** ER-2
- **Negative control:** SEALED (divergence, motion while locked); EXPLICIT/persistent (duplicate `unlocked`)
- **Production parity:** INC-5
- **Obligations:** OB-31, OB-3

### TST-22 — xmaximize (maximize and fullscreen answers)
- **Category / tier:** HARNESS / T1-style
- **Evidence:** ER-3
- **Negative control:** record-only override / sealed deferral (0 answers)
- **Production parity:** INC-7
- **Obligations:** OB-32, OB-22

### TST-23 — d5_model_1r (R1–R5)
- **Category / tier:** MODEL / T0-class
- **Evidence:** ER-5
- **Negative control:** sealed pan formula (R1); epoch disabled (R2); sealed re-fit (R3); Arrangement crossing while confined (R5)
- **Production parity:** INC-6, INC-10, INC-5
- **Obligations:** OB-23, OB-28, OB-31

### TST-24 — d5_model_2r (spatial-interaction epochs)
- **Category / tier:** MODEL / T0-class
- **Evidence:** ER2-2
- **Negative control:** 1R camera-only epoch rule (window drag: 39 changes)
- **Production parity:** INC-6, INC-14
- **Obligations:** OB-23, OB-28

### TST-25 — `tools/check-foundation-docs` (and `--self-test`)
- **Category / tier:** ENFORCE / T0
- **Evidence:** this package (D7 checker run)
- **Negative control:** seeded defects: duplicate `INV-*`, OB without pass criterion, OB-19 conditional, deferred item closed, duplicate namespace ownership, plus others listed by `--self-test`
- **Production parity:** INC-0 (CI)
- **Obligations:** OB-36
<!-- /living -->

## 4. Harness → production parity map (exit criteria)

When a PRODUCT increment introduces a surface already exercised in the harness, porting the listed test **and its negative control** to the production path is an exit criterion of that increment. Harness success is never accepted as proof of the production seam.

| Harness test(s) | Production surface | Increment |
|---|---|---|
| TST-09 xsnap / X-15, TST-10 xhit, TST-04 x6, TST-18 d5hit (zoom-1 configurations) | snapshot extraction → core → render list → DummyRenderer → physical-pixel hit-test | INC-1 |
| 2R output-lifecycle tests (TST-15 unit tests) | `output_connected`/`output_disconnected` hotplug and camera binding | INC-2 |
| TST-03 SC1/SC3, X5; TST-06 x9 | membership, frame clock, presentation feedback | INC-2 |
| TST-05 x8 | pointer ownership, CameraPan, KeepFocus | INC-3 |
| TST-07 x10, TST-20 xpopup_focus | keyboard ownership, ActiveElementFocus, popup routing | INC-4 |
| TST-05 x8 C3/C4, TST-08 x12, TST-21 xlock_grab, TST-23 R5 | pointer constraints, reconciliation, output removal | INC-5 |
| TST-16, TST-17, TST-23 R2, TST-24, TST-03 SC2 | ladder, ceiling, ScaleInteractionEpoch | INC-6 |
| TST-22 xmaximize | toplevel maximize/fullscreen answers, `activated` | INC-7 |
| TST-03 SC3, TST-06 x9, TST-11 (T1 part) | off-screen withhold / kick / fallback | INC-8 |
| TST-18 d5hit (zoomed configurations), TST-19, TST-23 R1 | zoomed navigation, F-PAN-1 | INC-10 |
| TST-14 c6-sat | satellite supervisor | INC-12 |
| TST-24 (WindowMove/Resize cases) | interactive move/resize epochs | INC-14 |
