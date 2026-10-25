# Permanent Engineering Knowledge System

## Overview
The **Permanent Engineering Knowledge System** (`docs/knowledge/`) is a structured, searchable, non-authoritative engineering memory for Infynitae Native. Its primary purpose is to preserve:
- Architectural lessons and design rationale.
- External compositor research (e.g. DriftWM) and graphics architecture studies (e.g. Microsoft DWM/DXGI/WDDM).
- Negative knowledge: approaches that were proposed, investigated, and rejected, along with the evidence justifying their rejection.
- Empirical benchmark data, reproduction test protocols, and mutation testing records.
- Engineering roadmaps and blueprints for upcoming milestones (e.g. Phase G native DRM/KMS).

---

## Where it sits

`docs/knowledge/` is the project's engineering memory: research, measurements, decisions and,
above all, approaches that were tried and rejected (`pitfalls/`), so they are not proposed again.
It explains; it does not set rules. The rules are the design spec in `docs/foundation/`
(invariants, state machines, formulas) and the behaviour choices in `docs/DECISIONS.md`; where a
record and those disagree, they win and the record should be corrected or marked `SUPERSEDED`.

```
knowledge/
├── INDEX.md          every record, by subsystem and by upcoming work
├── architecture/     how Core's model works, and why
├── research/         other compositors and graphics stacks (driftwm, DWM/DXGI)
├── decisions/        engineering resolutions and bug postmortems
├── experiments/      benchmarks and mutation-test records
├── pitfalls/         rejected approaches and the evidence against them
└── roadmap/          plans for later work
```

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
  - `APPROVED`: Decided, not yet built.
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
It checks unique record IDs, the metadata fields, the six sections, working relative links,
supersession links, that every record is listed in `INDEX.md`, and (outside CI) that no record
present on `main` was deleted: mark it `SUPERSEDED` instead. It needs only Python 3 and git.

CI runs `tools/verify-knowledge --content-only` (no git checks: CI's shallow checkout has no
`main`) and `--self-test`, which seeds nine defects one at a time and requires each to be caught.
Run the plain command locally before committing knowledge changes.
