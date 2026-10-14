# Contradictions, Disputed Claims, and Superseded Findings Ledger

This document maintains an explicit, traceable record of all findings that were initially reported, hypothesized, or proposed, but subsequently found to be **contradicted, factually obsolete, conflated, or superseded** by rigorous empirical testing and source audit.

---

## 1. Contradicted Findings (Factually Disproven)

### C-01: Claim that Infynitae Unconditionally Evicts Presenters on Pre-Map Grants (Finding A-3)
- **Original Claim:** AGY's audit (`driftwm_agy_audit.md` §3.3) asserted that under sealed `INV-PRES-1`, an unmapped newcomer requesting fullscreen immediately evicts the active presenter before drawing, requiring Infynitae to adopt DriftWM's queueing.
- **Contradiction:** Infynitae's active candidate branch `experimental/inc7-exit-readiness-2026-10-14` had **already resolved and verified this in commit [`07acfeb`](file:///home/vysaely/infynitae-native)**. In `07acfeb`, Core records the grant but defers output takeover until the newcomer commits its first buffer. Unmapped newcomers that die or unmap without drawing end nothing.
- **Authoritative Record:** [KNOW-DEC-001: Pre-Map Fullscreen Ownership](decisions/KNOW-DEC-001-a3-premap-fullscreen-ownership.md)

### C-02: Claim of Identical Focus Policy Loophole Behind Fullscreen (Finding A-8)
- **Original Claim:** AGY asserted that both DriftWM and Infynitae share an open policy loophole permitting keyboard focus to rest on occluded windows behind fullscreen presentations.
- **Contradiction:** DriftWM indeed retains this loophole ([`src/state/keyboard_focus.rs:106`](file:///home/vysaely/.gemini/antigravity/scratch/driftwm/src/state/keyboard_focus.rs#L106)). However, Infynitae **already resolved and optimized this in commits [`0fe8f06`](file:///home/vysaely/infynitae-native) and [`489180e`](file:///home/vysaely/infynitae-native)**. `hidden_by_presentation()` actively rejects focus moves to hidden windows, with an $\mathcal{O}(1)$ early-exit check when not presenting.
- **Authoritative Record:** [KNOW-DEC-002: Fullscreen Focus Eligibility](decisions/KNOW-DEC-002-a8-fullscreen-focus-eligibility.md)

### C-03: Recommendation to Query Host Monotonic Wall-Clock Time in Adapter (Finding A-5)
- **Original Claim:** AGY recommended querying `smithay::utils::Clock::<Monotonic>::new().now()` in `infynitae-wayland` for synthesized pointer events.
- **Contradiction:** Querying wall-clock time in the adapter creates a race condition with buffered `evdev` kernel events, causing **backward timestamp jumps** on the wire when queued events are dispatched. Infynitae rejected this and implemented `InputClock` in Core commit [`e40eb70`](file:///home/vysaely/infynitae-native), tracking the latest accepted hardware timestamp.
- **Authoritative Record:** [KNOW-ARCH-002: Monotonic Input Clock](architecture/KNOW-ARCH-002-monotonic-input-clock.md), [KNOW-PITFALL-004: Mixed Timestamp Clock Domains](pitfalls/KNOW-PITFALL-004-mixed-timestamp-clock-domains.md)

### C-04: Conflation of Initial Configure Bursts with Placement Acknowledgement (Finding P9b)
- **Original Claim:** AGY described Finding P9b as a mechanism to suppress multi-configure bursts on initial window creation to prevent SDL2/SCTK desyncs.
- **Contradiction:** In Infynitae governance and codebase, **P9b is lifecycle-based default placement after fullscreen exit upon acknowledging non-fullscreen state**, implemented in commit [`df72202`](file:///home/vysaely/infynitae-native). Initial configure burst suppression was an unrelated DriftWM startup optimization.
- **Authoritative Record:** [KNOW-DEC-003: Default Placement After Fullscreen Acknowledgement](decisions/KNOW-DEC-003-p9b-default-placement-acknowledgement.md)

### C-05: Uncritical Acceptance of DriftWM Focus Pull Benchmark ($5.5\,\mu\text{s}$)
- **Original Claim:** AGY claimed pulling pointer focus on every iteration costs $5.5\,\mu\text{s}$ over bare canvas and $<1\,\text{ms}$ CPU time per second.
- **Contradiction:** AGY never ran the benchmark and plagiarized documentation text. Independent host execution of `pull_cost_bench` revealed that bare canvas pull costs **$9.92\,\mu\text{s}$** (+80.4% slower) and real motion costs **$15.80\,\mu\text{s}$** (+95.1% slower). Under 8 kHz gaming mice, the pull model incurs quadratic $\mathcal{O}(N \cdot E)$ overhead.
- **Authoritative Record:** [KNOW-EXP-001: Independent Reproduction of Focus Pull Benchmark](experiments/KNOW-EXP-001-pull-cost-bench-reproduction.md), [KNOW-PITFALL-002: Global Focus Pull Loops](pitfalls/KNOW-PITFALL-002-global-focus-pull-loops.md)

### C-06: Misattribution of Smithay Pointer Handle Mutex Deadlock
- **Original Claim:** AGY claimed Smithay has a general pointer grab teardown deadlock requiring `insert_idle` ungrab.
- **Contradiction:** The deadlock in DriftWM was self-inflicted: DriftWM implemented panning as a custom Smithay `PointerGrab` (`PanGrab`) and called `pointer.unset_grab()` from inside its own button callback while `PointerInner`'s mutex was locked. Infynitae does not use a Smithay `PointerGrab` for panning and ungrabs via `&mut Inner<'_>` directly.
- **Authoritative Record:** [KNOW-RES-003: DriftWM Popups and Lock Reentrancy](research/KNOW-RES-003-driftwm-popups-and-locks.md)

---

## 2. Superseded Findings

### S-01: Pre-P1 Quadratic Scale Evaluation Defect (Commit `40ddd7d`)
- **Superseded Code:** Commit `40ddd7d` evaluated scale policy for every element on every commit, allocating 5,028 objects at 1,000 windows.
- **Superseding Code:** Commit [`f1ef262`](file:///home/vysaely/infynitae-native) repaired rule P-1, restricting scale passes to elements observed by an output. Allocations dropped to 15 per pan step at 1,000 windows.
- **Superseding Record:** [KNOW-ARCH-004: Spatial Membership Culling](architecture/KNOW-ARCH-004-spatial-membership-culling.md) (superseding [KNOW-PITFALL-005](pitfalls/KNOW-PITFALL-005-quadratic-scale-state-evaluation.md)).

### S-02: Initial Fullscreen Geometry Loss on Exit (Commit `782a3d3`)
- **Superseded Code:** In `782a3d3`, the saved size captured on fullscreen entry was the element's current geometry, which lost the windowed size if toggled rapidly.
- **Superseding Code:** Fixed in `77cde7f` via `restore_offers` in Core, decoupling saved windowed size from live committed geometry.
- **Superseding Record:** [KNOW-RES-002: DriftWM Fullscreen Lifecycle](research/KNOW-RES-002-driftwm-fullscreen-lifecycle.md)

---

## 3. Corrections Recorded by the INC-7 Exit Preparation (2026-10-14)

### X-01: "Focus on occluded windows is resolved" (C-02, KNOW-DEC-002, KNOW-ARCH-003) — partially corrected
- **Original Claim:** C-02 and the first versions of KNOW-DEC-002 / KNOW-ARCH-003 stated that Infynitae had resolved the loophole of keyboard focus resting on occluded windows, eliminating keystroke hijacking by invisible windows.
- **Correction:** A-8 refuses *moving* focus to a window hidden by a presentation. A window focused *before* it became hidden keeps the focus and keeps receiving keys (INC-7 exit review probes R1, R2): after a pre-map grant's element takes the output, and after the output that showed the focused window is removed. The owner kept this retention on 2026-10-14 (consistent with INV-INPUT-8 "visibility … never change it"); it is recorded as residual R-01.
- **Authoritative Record:** [KNOW-DEC-002](decisions/KNOW-DEC-002-a8-fullscreen-focus-eligibility.md) (rewritten), [KNOW-ARCH-003](architecture/KNOW-ARCH-003-presentation-visibility-gating.md) (corrected).

### X-02: "The 3 µs pan-step difference is a code-layout effect" — contradicted
- **Original Claim:** The performance-recovery checkpoint and the first exit-readiness record attributed the +3 µs pan step after `df72202` to code layout (alignment), without counting instructions.
- **Correction:** Instructions per pan step rose 9.9 % (547,821 → 601,815) on identical data and identical call counts; with `codegen-units = 1` both builds execute exactly 568,466 instructions per step. The cause is codegen-unit partitioning changing register allocation in `MembershipTable::with_output`, not alignment and not the P9b logic.
- **Authoritative Record:** [KNOW-EXP-005](experiments/KNOW-EXP-005-benchmark-codegen-unit-sensitivity.md).

### X-03: KNOW-ARCH-002 reproduction names and API — corrected
- **Original Claim:** KNOW-ARCH-002 cited `self.input.clock.latest()` and a test `audit_finding_a5_synthesised_pointer_events`.
- **Correction:** The method is `InputClock::wire_ms()`, read by `true_position()`; the T1 tests are `pointer_event_times_never_go_back_*`. The far-future timestamp residual was missing and is now recorded.
- **Authoritative Record:** [KNOW-ARCH-002](architecture/KNOW-ARCH-002-monotonic-input-clock.md).

### X-04: KNOW-DEC-003 hook ordering — corrected
- **Original Claim:** "The adapter extracts this from `current_state()` in the post-commit hook before invoking `CompositorHandler::commit`."
- **Correction:** Smithay's post-commit hook makes the acknowledged state current; the adapter reads it inside `CompositorHandler::commit`. Placement also requires the element to be out of fullscreen at that commit (probe R5).
- **Authoritative Record:** [KNOW-DEC-003](decisions/KNOW-DEC-003-p9b-default-placement-acknowledgement.md).

---

## 4. Reconciliation With Ratified INC-7 and the Original Research (2026-10-14, main `f323c2d`)

Earlier entries above are kept as written. Records were corrected by dated, struck-through or appended notes ("Reconciliation 2026-10-14"), not by rewriting their original facts.

### S-03: "UNRATIFIED" INC-7 status — superseded by ratification
- **Original:** KNOW-DEC-001..004, KNOW-ARCH-002, KNOW-ARCH-003, KNOW-DEC-005, KNOW-ROAD-005 and the INDEX status table said the INC-7 behaviour was unratified and layers C12.F5–F8 prepared.
- **Now:** C12.F5–F8 and C12.F10 adopted and INC-7 **ratified** (`39ad614`), main = origin/main = `f323c2d`. Accepted residuals R-01…R-15 are tracked follow-up work (KNOW-ROAD-005).

### X-05: Mutation-testing record — control count and mechanism wrong
- **Original:** KNOW-EXP-003: "over 700 seeded behavioral mutations", "temporary worktree", "fails with the expected failure diagnostic".
- **Correction:** 231 controls on main; a temporary directory copy; a control passes only if the named test fails as a test (compile errors and missing tests are broken controls); no diagnostic text is matched.

### X-06: Commit references point at source-line commits that are not on main
- **Original:** records and C-01..C-04 cite `07acfeb`, `0fe8f06`, `df72202`, `3b0a3d7`, `489180e`, `e40eb70`.
- **Correction:** none of these is on main. Main equivalents: A-3 `b1aa758`, A-8 `0f45cfd` + `2aa8d65`, P9b `334b9b3`, X-15 `23860b3` + `d350ed2`, A-5 `d70c3be` (KNOW-DEC-005 Facts 2 and 7). The original references stay valid as provenance.

### X-07: KNOW-ARCH-004 mixes main and INC-6
- **Original:** affected branches "main, …"; facts about scoped scale passes, the OB-42 ratchet and "< 31 µs pan at 3,000 windows".
- **Correction:** main has the visibility pass, bounding-box cull and binary-search lookup (`85a74cb`; still linear; pan 23.5 µs at 3,000 elements, T0 model). Scale-pass scoping, the ratchet (`f1ef262`) and `t0_hot_path_allocations` are INC-6 only, not ratified.

### X-08: Focus pull vs push — scaling claims unmeasured; Infynitae's model misdescribed
- **Original:** INDEPENDENT_REVIEW §6, DISPUTED_FINDINGS §5, KNOW-PITFALL-002, KNOW-EXP-001: pull is O(N·E) with "severe tail-latency spikes" at 8 kHz and N = 3,000; "Model B outperforms Model A by over 18×"; Infynitae is "O(1) steady-state", recomputing focus "only on boundary crossing"; DriftWM's numbers were "+80 % in error".
- **Correction:** only one 20-window run was measured (raw output archived). No harness or result for the 18× claim exists. On main, each motion is hit-tested by a linear scan of the output's culled `ProjectionFrame`, and Core emits true-position events at its own chokepoints (KNOW-RES-009). A single run on different hardware does not show DriftWM's documented numbers were wrong. The rejection of per-iteration pull stands on design grounds (Core is the chokepoint DriftWM lacks), not on a measured comparison. KNOW-PITFALL-002 evidence class: CONTRADICTED → DISPUTED.

### X-09: Wall-clock adapter timestamps — race reasoned, toolkit crashes unsourced
- **Original:** KNOW-PITFALL-004 Facts 2–3 and "confirmed on the wire with `fs_client`".
- **Correction:** `fs_client` evidence shows the A-5 fix, not the wall-clock race; the race is an argument from event ordering. "Qt, Chromium, GTK assert" has no source.

### X-10: Microsoft research — interpretations recorded as verified facts
- **Original:** KNOW-RES-004..007 "Verified Facts" (e.g. "exact architectural parity", "without kernel transitions", "50–100 ms", "DWM suspends its composition thread entirely", "kernel ≥ 6.8 / Mesa ≥ 24.1"); the source's Class 4 "Windows anti-patterns".
- **Correction:** no session fetched Microsoft documentation; the source cites page titles only. Each claim is now classed MS-DOC / INTERP / APPLY / WIN-ONLY in KNOW-RES-008. Class 4 items other than Blt-model presentation are DriftWM/AGY findings, not Windows mechanisms. "INC-8 must incorporate a gated heartbeat" is a candidate, not a requirement (INV-FRAME-1 governs).

### X-11: Microsoft research — statements about Infynitae
- **Original:** "INV-CAMERA-4 → no spurious damage on fullscreen outputs"; "Infynitae tracks damage deterministically per surface tree node"; KNOW-ARCH-001 "guarantees zero spurious damage propagation".
- **Correction:** INV-CAMERA-4 concerns configures and pointer events from a pan. Core has no damage model; damage is Smithay's, in the adapter and backends. No damage property is sealed or tested.

### X-12: KNOW-ARCH-001 — fullscreen projection is a zoom-1 view, not a fit to output bounds
- **Original:** "the presenting element is projected directly to output bounds at scale 1.0".
- **Correction:** F-PRESENT-1 is a zoom-1 view: a smaller surface is centred, a larger one origin-aligned and cropped.

### X-13: KNOW-PITFALL-006 — heartbeat cost on Infynitae
- **Original:** each off-screen commit "triggers damage re-evaluation, stage accounting, and scale passes"; ~3,000 commits per second.
- **Correction:** the rate is arithmetic, not measured; main has no scale policy (INC-6 unratified) and scoped visibility work. The direction (gate any heartbeat) stands; R-10 assigns frame callbacks for hidden windows to INC-8.

### X-14: `tools/verify-knowledge` reported foundation edits that did not exist
- **Original:** the sealed-file check diffed the working tree against the moving `main` ref. After main advanced to `f323c2d`, the knowledge branch (based on `ccb82c5`) failed with 14 "Sealed foundation file modified" errors although it touches no foundation file.
- **Correction:** the check now diffs against the merge base with main (what this branch changed). It also now checks that no record present at the merge base was removed, that every record is listed in INDEX.md, and that every `superseded_by` target exists.

### X-15: Label errors kept for provenance
- KNOW-RES-001 linked "INDEPENDENT_REVIEW.md" to KNOW-EXP-001 (fixed: now names the archived file). KNOW-RES-002, KNOW-ARCH-003 and KNOW-ARCH-004 list, under "Unverified Hypotheses", statements they then call verified; read those lines as facts with the cited test. Not rewritten.

### X-16: KNOW-PITFALL-003 — severity overstated
- **Original:** an invalid-serial grab lets a client "monopolize seat pointer and keyboard input"; "mathematically obvious".
- **Correction:** the grab ends when the user clicks outside the client's surfaces; the effect is focus stealing until dismissed. No adversarial script exists. Infynitae's rejection (W-SM-7) is unchanged.

### G-01: Coverage gap — DriftWM camera and viewport
- **Finding:** no recovered study covers DriftWM's camera, viewport, coordinate types or "constant-time camera movement" beyond one audit table row and DriftWM's own documentation. KNOW-RES-009 records what is known and the gap. ~~Not searched: the old Arch partition.~~
- **Update 2026-10-15 (old Arch partition):** the old installation (btrfs `f65d4194-ff71-4906-9578-b33bfd3b2688`, then on the 465.8 GB Kingston SNV2S500G NVMe) was last surveyed read-only on 2026-10-05 (Claude session `d0318fbc`). Its filesystem is no longer present: that disk now carries a Windows layout (16 MB MSR + NTFS) and no attached device has that UUID. It could not have held the research anyway: DriftWM first appears on this machine on 2026-10-14 (AGY trajectory `c54deef2`, whose brief told it to clone DriftWM fresh; its first search of `$HOME` found no DriftWM), nine days after the old installation's last use. The 2026-10-05 survey listed no DriftWM material. **Conclusion:** the AGY audit is the original DriftWM research; no camera/viewport study was lost. The gap is real, not a recovery failure. A bounded source-study plan is in KNOW-RES-009; it has not been executed.
