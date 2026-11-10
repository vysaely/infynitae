# Agent Integration Proposal: Directing Autonomous Coding Agents

**Document:** Proposed Agent Integration Directives (`AGENTS.md` / `CLAUDE.md`)  
**Status:** PROPOSED — PENDING OWNER APPROVAL. Reconciliation 2026-10-14: a one-item pointer (item 7, "Read, in this order") was added to `docs/archive/development/START_HERE_FOR_AGENT.md` on `knowledge/integration-2026-10-14` in its own commit, so it can be dropped on its own; the longer text below stays a proposal.  
**Context:** Guidelines for AGY, Claude Code, Codex, and human developers to consult and maintain the Knowledge System.

---

## 1. Minimal Proposed Additions to `docs/archive/development/START_HERE_FOR_AGENT.md`

Add the following section under *"Read, in this order"*:

```markdown
### 7. Consult the Engineering Knowledge System (`docs/knowledge/INDEX.md`)
Before designing, proposing, or refactoring compositor architecture:
1. **Check Prior Art:** Read `docs/knowledge/INDEX.md` to determine if the problem, subsystem, or optimization was previously investigated.
2. **Review Negative Knowledge:** Always consult `docs/knowledge/pitfalls/` to verify that your proposed optimization was not already investigated and rejected (e.g. focus pull loops, physical canvas relocation, wall-clock timestamps).
3. **Verify Branch Applicability:** Knowledge records record exact commits and branches. Never assume a candidate branch optimization is merged into `main` without checking `git log`.
4. **Maintain Separation of Governance:** `docs/foundation/` is sealed and authoritative. `docs/knowledge/` is non-authoritative memory and cannot modify or override sealed invariants.
```

---

## 2. The 8 Core Rules for Coding Agents

1. **Read `docs/knowledge/INDEX.md` First:** When assigned an architectural, performance, or protocol task, start by reading the knowledge index.
2. **Check Relevant Existing Knowledge:** Do not write fresh design plans from scratch without cross-referencing existing records in `architecture/`, `research/`, or `decisions/`.
3. **Check Sealed Governance Separately:** Always consult `docs/archive/CHECKPOINT.md` and `FOUNDATION_INVARIANTS.md` for current ratified gates. Knowledge records are descriptive, not legislative.
4. **Verify Current Branch Applicability:** Before citing a past audit finding, inspect `git log` and `git status` on the active working tree to ensure the finding applies to the current commit.
5. **Consult Rejected and Superseded Approaches:** Do not re-propose discarded patterns (such as unconditional focus pulls or bogus grab serial acceptance) without providing new empirical counter-evidence.
6. **Update Knowledge When Evidence Changes:** When a new benchmark or experiment invalidates an existing record, update the record's `evidence_class` and status, or create a superseding record.
7. **Preserve Benchmark Provenance:** Always record exact compiler profiles, iteration counts, hardware specs, and warm-up parameters when recording performance claims.
8. **Avoid Duplicate Research Documents:** Do not create scattered, redundant markdown files across scratch directories. All permanent engineering insights belong in `docs/knowledge/`.
