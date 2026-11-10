# Keeping the knowledge records

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
- **Verification Automation:** Running `tools/verify-knowledge` . Run it prior to committing documentation changes.
- **Minimal Template:** Copy an existing record from the target category as a starting template.
