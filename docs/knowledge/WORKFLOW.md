# Engineering Knowledge System Maintenance & Development Workflow

## Overview
This document defines the operational lifecycle for engineering research, feature development, defect resolution, and knowledge maintenance in Infynitae Native.

The goal is to maintain a high-signal, evidence-driven engineering memory with **minimal friction** during routine development, while enforcing strict evidence standards for architectural changes and performance claims.

---

## The 7-Stage Engineering Development Lifecycle

```
[ 1. Research & Problem Definition ]
                  │
                  ▼
[ 2. Hypothesis & Metric Formulation ]
                  │
                  ▼
[ 3. Empirical Experiment / Proof-of-Concept ]
                  │
                  ▼
[ 4. Independent Adversarial Review ]
                  │
                  ▼
[ 5. Sealed Governance Ratification ] (docs/foundation/)
                  │
                  ▼
[ 6. Implementation & Mutation Validation ]
                  │
                  ▼
[ 7. Knowledge Record Ratification & Indexing ] (docs/knowledge/)
```

### Stage 1: Research & Problem Definition
- Identify the subsystem, protocol boundary, or hardware interaction requiring investigation.
- Review existing knowledge records in `docs/knowledge/` to prevent re-litigating known pitfalls or duplicating prior research.
- Review sealed invariants in `docs/foundation/` to ensure problem scope does not violate fundamental architectural constraints.

### Stage 2: Hypothesis & Metric Formulation
- State the proposed design change or optimization as an explicit testable hypothesis.
- Define quantitative success metrics (e.g. allocation count == 0 under `OB-42`, dispatch latency < $5\,\mu\text{s}$, zero protocol error events under `verify-test-efficacy`).
- Formulate the expected failure modes and potential trade-offs.

### Stage 3: Empirical Experiment / Proof-of-Concept
- Create an isolated branch or worktree (`research/<topic>` or `experiment/<topic>`).
- Implement the minimal differential change.
- Run benchmarks with warm-up cycles, isolated CPU affinity, and statistical significance analysis.
- Create automated negative tests that fail if the optimization degrades correctness or drops events.

### Stage 4: Independent Adversarial Review
- Subject the findings and code to independent review before merging or adopting claims.
- Verify:
  1. Exact git revisions and build profiles (`--release`, `lto = "thin"`).
  2. Protocol compliance (e.g. Wayland xdg-shell commit/configure/ack sequencing).
  3. Methodological validity (e.g. allocation accounting via `OB-42`, clock domains).
  4. Absence of GPL or proprietary contamination.
- Output an adversarial assessment classifying claims (`VERIFIED`, `CONTRADICTED`, `DISPUTED`, etc.).

### Stage 5: Sealed Governance Ratification
- If the change alters crate boundaries, core invariants, or state machine transitions, update the authoritative governance documents in `docs/foundation/`:
  - `FOUNDATION_INVARIANTS.md` (e.g. adding or tightening an invariant).
  - `FOUNDATION_ARCHITECTURE.md` (e.g. updating the crate dependency DAG).
  - `PROTOTYPE_DECISION_LEDGER.md` (recording the ratified architectural choice).
- Any foundation change must pass `tools/check-foundation-docs`.

### Stage 6: Implementation & Mutation Validation
- Implement the solution on the targeted increment branch.
- Verify that unit tests, integration tests, and counting allocators pass cleanly.
- Run mutation testing (`tools/verify-test-efficacy` or equivalent) to verify that tests are sensitive to regressions and negative conditions.

### Stage 7: Knowledge Record Ratification & Indexing
- Author or update the corresponding record in `docs/knowledge/`.
- Ensure all required metadata fields and the 6 mandatory sections are present.
- Run `tools/verify-knowledge` to ensure schema compliance and link integrity.
- Update `docs/knowledge/INDEX.md` and related ledgers.

---

## When to Create a Knowledge Record

A new knowledge record is **required** when:
1. **New Invariant or Architecture Principle:** An architectural rule or system property is established that spans multiple crates or commits (record type: `architecture/`).
2. **External Architecture Research:** An investigation of an external codebase, window manager, compositor, or OS graphics architecture is completed (record type: `research/`).
3. **Protocol Defect or Non-Obvious Decision:** A non-trivial bug, race condition, or protocol sequencing trap is resolved (record type: `decisions/`).
4. **Empirical Benchmark or Experiment:** An empirical test, performance comparison, or validation protocol is established (record type: `experiments/`).
5. **Rejected Approach (Negative Knowledge):** An architectural direction, proposed optimization, or plausible alternative is investigated and rejected (record type: `pitfalls/`).
6. **Future Roadmap / Milestone Blueprint:** Design principles and hardware interfaces for future development phases are specified (record type: `roadmap/`).

Routine refactorings, minor typo fixes, and internal implementation details that do not affect invariants or architectural boundaries **do not** require knowledge records.

---

## Updating Existing Records Without Historical Loss

Knowledge records represent the state of knowledge at a specific point in time, but must evolve as new experiments occur:

1. **Do Not Rewrite History:** Preserve the original `source_rev`, `date`, and initial verified facts.
2. **Append Evidence:** When new benchmarks or independent reproductions occur, append a new subsection in `## Verified Facts` or `## Reproduction Instructions` with the date, commit SHA, and new measurement.
3. **Update Evidence Classification & Adoption Status:** Modify `evidence_class` (e.g. from `HYPOTHESIS` to `EXPERIMENTALLY VERIFIED`) and `adoption_status` (e.g. from `RESEARCH` to `IMPLEMENTED`).
4. **Note Conditions for Reconsideration:** If a rejected pitfall (`pitfalls/`) becomes viable due to upstream changes (e.g. a new Linux kernel API or Smithay release), document the exact prerequisite conditions in `## Limitations & Counterexamples`.

---

## Marking Superseded Knowledge

When a finding or design is rendered obsolete by a superior architecture:
1. Set `adoption_status: SUPERSEDED` in the metadata table.
2. Add the metadata field `superseded_by: KNOW-XXX-YYY` pointing to the replacement record.
3. In `## Verified Facts`, summarize what was replaced and link to the new record.
4. Record the supersession in `docs/knowledge/CONTRADICTIONS_AND_SUPERSEDED.md`.
5. Run `tools/verify-knowledge` to ensure the supersession link resolves bidirectionally.

---

## Fast-Path for Routine Development

To maintain engineering velocity and keep documentation overhead low:
- **Batching:** Knowledge records may be authored at the conclusion of an increment or research milestone rather than per-commit.
- **Verification Automation:** Running `tools/verify-knowledge` takes < 1 second. Run it prior to committing documentation changes.
- **Minimal Template:** Copy an existing record from the target category as a starting template.
