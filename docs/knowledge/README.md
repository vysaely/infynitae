# Permanent Engineering Knowledge System

## Overview
The **Permanent Engineering Knowledge System** (`docs/knowledge/`) is a structured, searchable, non-authoritative engineering memory for Infynitae Native. Its primary purpose is to preserve:
- Architectural lessons and design rationale.
- External compositor research (e.g. DriftWM) and graphics architecture studies (e.g. Microsoft DWM/DXGI/WDDM).
- Negative knowledge: approaches that were proposed, investigated, and rejected, along with the evidence justifying their rejection.
- Empirical benchmark data, reproduction test protocols, and mutation testing records.
- Engineering roadmaps and blueprints for upcoming milestones (e.g. Phase G native DRM/KMS).

---

## Authority & Separation of Concerns

```
docs/
├── foundation/           <-- AUTHORITATIVE & SEALED GOVERNANCE
│   ├── CHECKPOINT.md     (Current ratified increment gate)
│   ├── FOUNDATION_INVARIANTS.md (Mathematical & architectural invariants)
│   ├── FOUNDATION_ARCHITECTURE.md (Crate topology, DAG, namespace maps)
│   └── PROTOTYPE_DECISION_LEDGER.md (Ratified project decisions)
│
└── knowledge/            <-- NON-AUTHORITATIVE ENGINEERING MEMORY
    ├── INDEX.md          (Master taxonomy & record directory)
    ├── architecture/     (Core architectural invariants & proofs)
    ├── research/         (External systems research & audits)
    ├── decisions/        (Engineering resolutions & bug postmortems)
    ├── experiments/      (Empirical benchmarks & mutation test harnesses)
    ├── pitfalls/         (Negative knowledge & rejected anti-patterns)
    └── roadmap/          (Future milestone blueprints)
```

> [!IMPORTANT]
> **Authority Invariant:** Files in `docs/knowledge/` are strictly non-authoritative. They may document, explain, and cross-reference sealed foundation requirements, but they **cannot modify, supersede, or override** any document in `docs/foundation/`.

---

## Knowledge Record Schema

Every individual record in `docs/knowledge/` follows a standard schema validated by `tools/verify-knowledge`:

### 1. Header Metadata Table
- `id`: Stable identifier (format: `KNOW-<CATEGORY>-<NUM>`, e.g. `KNOW-ARCH-001`).
- `title`: Concise summary of the record.
- `subsystem`: Primary engineering subsystem (e.g. `Core/Presentation`, `Wayland/Popups`, `Backend/DRM`).
- `source_rev`: Commit SHA or upstream URL where the finding originated.
- `date`: ISO 8601 date (`YYYY-MM-DD`).
- `evidence_class`: Allowed enum:
  - `SOURCE-DERIVED`: Extracted from code analysis.
  - `EXPERIMENTALLY VERIFIED`: Validated by automated tests.
  - `INDEPENDENTLY REPRODUCED`: Confirmed by independent execution on host hardware.
  - `HYPOTHESIS`: Theoretical proposition pending measurement.
  - `DISPUTED`: Contradicted by alternative evidence or incomplete tests.
  - `CONTRADICTED`: Formally disproven by empirical evidence.
- `applicability`: Current applicability to the codebase.
- `affected_branches`: Specific branches affected.
- `related_invariants`: Related foundation invariants (e.g. `INV-PRES-1`, `OB-42`).
- `adoption_status`: Allowed enum:
  - `RESEARCH`: Candidate for investigation.
  - `PROPOSED`: Proposed for implementation.
  - `APPROVED`: Ratified by governance.
  - `IMPLEMENTED`: Landed in codebase.
  - `VALIDATED`: Verified via automated mutation tests.
  - `REJECTED`: Formally discarded.
  - `SUPERSEDED`: Replaced by a newer finding (`superseded_by` required).
  - `DEFERRED`: Postponed to a future milestone.
- `next_action`: Concrete next experiment or verification action.

### 2. Standard Content Sections
Every record must contain the following six level-2 markdown sections:
1. `## Verified Facts`
2. `## Unverified Hypotheses`
3. `## Performance & Correctness Implications`
4. `## Limitations & Counterexamples`
5. `## Reproduction Instructions`
6. `## Links & Citations`

---

## Verification & Tooling

The integrity of the knowledge system is enforced automatically by:
```bash
tools/verify-knowledge
```
The script validates record ID uniqueness, metadata completeness, link integrity, and ensures zero accidental modifications to `docs/foundation/`.

Since 2026-10-14 it also: compares `docs/foundation/` with the merge base of `HEAD` and `main` (so main advancing is not reported as an edit); fails if a record present at that merge base was removed (mark it `SUPERSEDED` instead); fails if a record is not linked from `INDEX.md`; and checks relative links in the meta documents. It needs no network, service or dependency beyond Python 3 and git.

CI (`.github/workflows/ci.yml`, job `foundation-docs`) runs `tools/verify-knowledge --content-only` and `tools/verify-knowledge --self-test` on every push and pull request. `--content-only` skips the two merge-base checks: CI's shallow checkout has no `main`, and governance branches change `docs/foundation/` legitimately (guarded there by `tools/check-foundation-docs`). A deleted indexed record still fails through `INDEX.md`'s broken link. `--self-test` seeds nine defects one at a time into a temporary copy and requires each to be detected. Run the plain command locally before committing knowledge changes.
