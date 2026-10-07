# Phase F INC-4 — Fifth Independent Adversarial Review Dossier

This dossier contains the independent adversarial review of Phase F INC-4 Round-4 corrective program.

- **Reviewed commit:** `4c01cca2d43e38f4f3a80e9202ec1b7ab0430179` (= `origin/main`)
- **Baseline:** `1ee520fa0079d45193f2a7274b31018e9276b0ed` (Ratified Phase F INC-3)
- **CI run:** GitHub Actions Run ID `37476629305` on exact HEAD `4c01cca` (green)
- **Verdict:** `INC-4 PASS — READY FOR FORMAL RATIFICATION`

## Structure

- [`VERDICT.md`](VERDICT.md): Definitive verdict, summary of previous findings, closure analysis, and obligation disposition.
- [`POPUPS_AND_PROVENANCE.md`](POPUPS_AND_PROVENANCE.md): Deep evaluation of `reconcile_popup_chain`, depth-independence (depths 1..5), `toplevel_tree_changed`, lifecycle separation, and the 64-entry FIFO provenance store.
- [`AUTHORITY_AND_FOCUS.md`](AUTHORITY_AND_FOCUS.md): Capability sealing of `InputAuthToken`, event-binding, duplicate transition gate centralization, and Core/Smithay focus coherence.
- [`KEYBOARD_AND_MODIFIERS.md`](KEYBOARD_AND_MODIFIERS.md): Modifier transitions across keys/modifiers/contexts, canonical binding policies, and K6 cancel semantics.
- [`OBLIGATION_REVIEW.md`](OBLIGATION_REVIEW.md): Formal analysis of OB-4, OB-15, OB-18, OB-21, and OB-30.
- [`REGRESSION_AND_RESOURCES.md`](REGRESSION_AND_RESOURCES.md): Ratified INC-3 regression surface, CameraPan, KeepFocus, true-position motion, and 200/250-cycle stress evaluations.
- [`ADVERSARIAL_ATTACKS.md`](ADVERSARIAL_ATTACKS.md): 22 fresh mutations, attack methodology, detection records, and semantic equivalence proofs.
- `reproduction/`: Independent attack suite source, fresh mutation output, derived counts, and CI logs.
