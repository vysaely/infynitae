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
