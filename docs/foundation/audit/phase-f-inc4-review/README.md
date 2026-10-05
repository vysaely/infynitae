# Phase F Increment 4 (INC-4) — Independent Adversarial Systems Architecture Review Dossier

- **Review target:** `~/infynitae-native`
- **Reviewed HEAD:** `d5b7b95cbc910c48377c72ae94ccec2b0c05e140` (origin/main)
- **Pre-INC-4 baseline:** `1ee520fa0079d45193f2a7274b31018e9276b0ed` (INC-3 ratified)
- **Category:** PRODUCT, HEADLESS
- **State:** complete, untracked review artifacts. Production implementation untouched; all experiments ran in scratch worktrees.

## Dossier
1. [VERDICT.md](VERDICT.md) — verdict, findings catalog, verification matrix, required actions.
2. [RECONSTRUCTION.md](RECONSTRUCTION.md) — integrity, independent scope reconstruction, canonical contradictions, diff audit.
3. [KEYBOARD_AND_FOCUS.md](KEYBOARD_AND_FOCUS.md) — K1–K9, M12, modifier authority, bindings, OBJ-22/M11, fallback, focus cycling, INV-PROTO-1.
4. [POPUPS_AND_PREEMPTION.md](POPUPS_AND_PREEMPTION.md) — pinned Smithay facts, INV-INPUT-10, popup grab model, W-SM-7/14/15, OB-4, D-B6-BOUNDARY rule 1, zero outputs.
5. [ADVERSARIAL_ATTACKS.md](ADVERSARIAL_ATTACKS.md) — 18 attacks + resource stress, 29 mutations, oracle independence, count reconciliation, INC3-F06/F09.
6. [OBLIGATION_REVIEW.md](OBLIGATION_REVIEW.md) — obligation dispositions, authority/negative-ownership audit, gaming safety.
7. [reproduction/](reproduction/) — attack suite, mutation runner, logs, concurrency report.

## Summary
- Level A 1 (INC4-F01), Level B 11, Level C 4, Observations 6.

```
INC-4 FAIL — ARCHITECTURAL CORRECTION REQUIRED
```

Safety: no live compositor activation, no DRM/KMS, no VT switch, no boot/package changes, old Arch partition untouched. INC-5 not started.
