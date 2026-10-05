# E1R — D8 implementation-plan re-audit

Status: **UNSEALED**. Scope: only dependencies, gates or increments invalidated by E1R. The canonical plan is FOUNDATION_IMPLEMENTATION_PLAN.md (INC-0 … INC-17, gates G-OB24, G-OB34); Phase E's summary is not used as the source.

## Checklist (E1R authorization §21)

| Check | Result | Where |
|---|---|---|
| INC-0 remains enforcement-only | **holds**: unchanged (ENFORCE; OB-19 + OB-36). OB-19 gains probes p10–p13/2h/2i, which are enforcement artefacts | INC-0; C12.E1R.4 |
| OB-19 remains unconditional | **holds**: Status OPEN, Destination PHASE-F-ENTRY-GATE, Blocks "unconditional"; the checker's `ob19-conditional` passes | TEST_PLAN OB-19 |
| no product behaviour enters INC-0 | **holds**: no E1R item is assigned to INC-0 | — |
| INC-1 does not depend on unresolved Level-A architecture | **holds**: INC-1 (one output, Normal transform, thin seam) touches none of E-F2-02 (constraints: INC-5) or the rotated-output path (INC-2/3); F-HIT-1's corrected form is identical at Normal. E1R-N1 is corrected in the architecture | INC-1 unchanged |
| rotation coverage precedes rotated-output claims | **added**: OB-45 (T1) is an INC-2/INC-3 exit item and blocks any rotated-output product claim; INC-13 adds a NESTED GLES rotated-output check; OB-46 (T4) blocks release claims | INC-2, INC-3, INC-13; OB-45, OB-46 |
| dynamic output reconfiguration has an explicit increment/gate | **added** to INC-2 objective and exit tests (OB-49); probes p10–p13 | INC-2 |
| zero-output behaviour tested before multi-output lifecycle is complete | **added**: OB-50 core part in INC-2, pointer part in INC-3, constraint part in INC-5; INC-5 exit blocked by OB-50 | INC-2, INC-3, INC-5 |
| constraint regression accompanies the production constraint surface | **added**: OB-47 (EE-2 sequences) and OB-51 are INC-5 exit items; the OB-11 decision now precedes shipping **any** constraint behaviour | INC-5 |
| output persistence uncertainty gated | **added**: gate G-OB48 (no cross-session persistence claim before OB-48); in-session behaviour unaffected | gates |
| no open architecture decision crossed accidentally | **holds**: G-OB24, G-OB34 unchanged; new gates G-OB48, G-MIRROR; OB-11 decision point moved earlier, not removed; CP2-Q1 remains open (E-F2-07) | gates |
| harness evidence not relabelled as production evidence | **holds**: EE-1…EE-7 are HARNESS/MODEL/SPIKE; every new OB keeps a T1/T4/DECISION destination and stays OPEN | EVIDENCE §10; OB-45…51 |

## Increment changes

| Increment | Change | Reason |
|---|---|---|
| INC-2 | objective adds `output_reconfigured`, 1:1 binding + collision rule, `view_O` membership, `phys_O` trackers, zero-output core state; obligations OB-45 (part), OB-49, OB-50 (core); exit tests and negative controls added | E-F2-01, E-F2-03, E-F2-04, E-F2-05, E-F3-03 |
| INC-3 | obligations OB-45 (hit part), OB-50 (pointer part); exit tests EE-1 C5 and the zero-output pointer transition | E1R-N1, E-F2-05 |
| INC-5 | OB-11 decision before any constraint ships; OB-47, OB-50 (constraint part), OB-51; EE-2/EE-3 exit tests; negative controls on the published dependency | E-F2-02, E-F2-05, E1R-N2 |
| INC-6 | OB-13 now includes the zero-extent domain and integer bound; EE-5 properties as exit tests | E-F2-06, E1R-N3 |
| INC-13 | NESTED GLES rotated-output check | E-F2-01 |
| gates | G-OB48, G-MIRROR added | E-F3-03, E-F2-04 |

No increment added, removed or renumbered. Increment count stays 18 (INC-0 … INC-17).
