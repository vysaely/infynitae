# INC-6 exit — adoption package (prepared 2026-10-16, revision 2; NOT ADOPTED)

Status: **PREPARED FOR THE OWNER'S DECISION. Nothing here is adopted.** Each item below is one
decision, with the exact text that a governance commit would apply if the owner approves it.
`<DATE>` and `<BASE>` are filled at adoption, never before. No invariant is added or removed
(57 stays 57). Evidence: [EXIT_REVIEW_R2_2026-10-16.md](EXIT_REVIEW_R2_2026-10-16.md).

`tools/check-foundation-docs` accepts a layer only with a `Status: ADOPTED <date>` line, so the
layer sections cannot sit in FOUNDATION_ARCHITECTURE before adoption without asserting an
adoption that has not happened; they are kept here.

**Revision 2 (2026-10-16, third brief).** Two texts of revision 1 were corrected to the code of
`397ac72`: the redraw that spends a restore offer is defined by the window-geometry size (P-1);
C12.F12's prediction is geometric over each other output's current view, not "exactly the
presentations that observe the element" (P-2). One **code defect** was found against C12.F13's
text and fixed (r3 review, EXIT_REVIEW_R2 §8): the sweep's "re-evaluate everything" flag, set by
a device-limit report or a configuration change, was read as "a system change touched every
element", so the next unrelated stimulus (a camera drag elsewhere) released every hold. P-3's
rule — only output changes touch every element — is now what the code does. P-6 adds a mechanical
check that keeps OB-42's evidence as strong as "a test in core" was.

## Decision sheet

| # | Decision | Recommendation | Effect if approved |
|---|---|---|---|
| D1 | Adopt layer C12.F11 with the text of P-1 | approve | F-SCALE-2's `s_max(E)` counts sizes Core asks for |
| D2 | Adopt layer C12.F12 with the text of P-2 | approve | F-SCALE-3 / INV-SCALE-5 initial hint of a pre-map grant |
| D3 | Adopt layer C12.F13 with the text of P-3 | approve | C14.4 gains the presentation-hold paragraph; INV-SCALE-5 one exception |
| D4 | Record C12.F9 withdrawn (P-4) | approve | correction of the 2026-10-14 decision; number stays unused |
| D5 | Split OB-28 into OB-28 + OB-53 (P-5) | approve | OB-28 can close at the INC-6 exit; OB-53 carries the rest |
| D6 | Correct OB-42's closure wording (P-6) | approve | OB-42 can close at the INC-6 exit on its present evidence |
| D7 | Close OB-13, OB-28, OB-42 at the exit; keep OB-44, OB-52, OB-53 open | approve after D1–D6 | INC-6 exit criteria met |

Each of D1–D6 can be approved or refused alone; D7 needs D1 (OB-13) and D5 (OB-28) and D6
(OB-42).

---

## P-1. §C12.F11 — the device bound counts a size Core asks for (OB-13)

Place after §C12.F10 (layers are ordered by adoption).

> **Status: ADOPTED `<DATE>` by owner approval.** F-SCALE-2's `s_max(E)` also counts every size
> Core has asked E for while that request stands. One definition of one formula changes; no
> invariant is added or removed (57). Base: `<BASE>`.
>
> **Inputs:** F-SCALE-2 (C12.F2) as effective at the base; C12.F6 INV-PRES-1 and C5.2 (a grant's
> configure asks for the output's logical size); M9 (the exit configure offers the saved size);
> OB-13's measurement (`D_max` 8192, scale-3 observer, 3840 px output: a first fullscreen buffer of
> 11,520 px); the restore-offer case (3000 px window, hint 2, `D_max` 4096).
>
> **Provenance:** `evidence/phase-f/INC-6/C12F11_PROPOSAL.md` (corrected 2026-10-16),
> `evidence/phase-f/INC-6/EXIT_REVIEW_R2_2026-10-16.md`.
>
> F-SCALE-2, the sentence defining `s_max(E)` (living §F carries the corrected text):
>
> `s_max(E)` = the largest logical side over all surfaces of E's tree and over every size Core
> has asked E for while that request stands: the size of the protocol-fullscreen configure while
> E is granted (C5.2: the output's logical size; at the initial commit, of the grant made in the
> initial configure), and the saved size offered in the configure that ended E's fullscreen (M9),
> until E commits a tree whose window-geometry size differs from the one it replaces, is
> unmapped, or is destroyed (0 iff none has a non-zero extent, e.g. every initial commit without
> a grant).
>
> **C12.F11.1 Superseded:** F-SCALE-2 (C12.F2) "`s_max(E)` = largest logical side over all
> surfaces of E's tree (0 iff no surface has a non-zero extent, e.g. every initial commit)" →
> F-SCALE-2 (C12.F11).
>
> **C12.F11.2 What this layer does not change:** the ladder, `H_cfg`, P7's trigger and effect, the
> integer emission rule, SC-E, the epochs. A client-chosen size (a windowed resize) still enters
> only with the buffer that has it. M15's row "first buffer attached (`s_max` becomes > 0)" gains
> "or a size Core asks for". A re-grant made while a size is on offer saves that size (the last
> windowed size), not the fullscreen geometry. OB-13 is not closed by this layer.
>
> Tests (T0, `infynitae-core`): `a_grant_never_asks_for_a_buffer_over_the_device_limit`,
> `a_pre_map_grant_initial_hint_fits_the_device_limit`,
> `the_size_offered_at_fullscreen_exit_fits_the_device_limit`,
> `a_spent_restore_offer_no_longer_bounds_the_hint`, `an_unmap_spends_the_restore_offer`,
> `a_re_grant_before_the_redraw_offers_the_windowed_size_again`; seeded controls "C12.F11: …".

Living §F: replace the `s_max(E)` sentence of F-SCALE-2 with the text above and add "C12.F11"
to its **Amended** line. FOUNDATION_INVARIANTS INV-SCALE-2: layers `… → C12.F2 → C12.F11`.

## P-2. §C12.F12 — the initial hint of a pre-map fullscreen grant

> **Status: ADOPTED `<DATE>` by owner approval.** One clause is added to the initial hint of
> F-SCALE-3 and INV-SCALE-5; no invariant is added or removed (57). Base: `<BASE>`.
>
> **Provenance:** `evidence/phase-f/INC-6/C12F12_PROPOSAL.md` (corrected 2026-10-16).
>
> F-SCALE-3 and INV-SCALE-5, **Initial hint**, add: "If the initial configure grants protocol
> fullscreen on output O (C12.F6), ρ_pred = max(`scale_O`, F-SCALE-1 over the current views of
> the other outputs whose view intersects the placed window-geometry rect, or the anchor point
> when its size is 0×0). The grant's own presentation contributes `scale_O` at zoom 1; O's
> current view does not count."
>
> **C12.F12.1 Superseded:** F-SCALE-3 / INV-SCALE-5 (C12.E1R) initial hint, as far as it applies to
> an element with a pre-map grant.
>
> **C12.F12.2 What this layer does not change:** the prediction of every element without a
> pre-map grant; its geometric character (a view intersecting the placed rect counts even if, at
> the first map, another element's presentation hides the new one there: the first-map
> evaluation corrects such an over-prediction, as for any element); M15's order (hint before
> the initial configure); the evaluation at the first map.
>
> Tests (T0): `a_pre_map_grant_takes_the_presenting_outputs_scale_when_it_maps`,
> `a_pre_map_grant_initial_hint_fits_the_device_limit`.

## P-3. §C12.F13 — whose stimulus a presentation change is

> **Status: ADOPTED `<DATE>` by owner approval.** C14.4 gains one paragraph and INV-SCALE-5 one
> exception; no invariant is added or removed (57). Base: `<BASE>`.
>
> **Provenance:** `evidence/phase-f/INC-6/C12F13_PROPOSAL.md` (corrected 2026-10-16).
>
> C14.4, after the list of non-client stimuli of SC-E:
>
> Fullscreen enter/exit is a non-client stimulus for the element whose presentation starts or
> ends (its subject); its open epoch settles (M9 "settlement"). For every other element whose
> observers change only because a client's request, commit or destruction started or ended a
> presentation, the change may raise its hint (as a non-client stimulus) but never lowers it: the
> element **holds** its hint against decreases until a non-client stimulus that is not such a
> change and that **touches** the element evaluates it, or until an evaluation finds its
> requirement back at or above its hint. Touching: an output added, removed or reconfigured
> touches every element; a camera action touches the elements that an output bound to the moved
> camera observed before it or observes after it; an element's own fullscreen enter/exit touches
> it. A reported device limit (`D_max`) or a change of the scale configuration touches no element:
> P7 applies at once, and the re-evaluation they cause releases no hold. A settlement does not
> lower a held element its stimulus never touched. P7 applies regardless. A presentation the
> system ends or starts (output added, removed, reconfigured) is a non-client stimulus for every
> element.
>
> INV-SCALE-5 gains: "… equals Phase B MaxGesture, except that an element held under C14.4
> (another client's presentation) keeps a higher hint until a non-client stimulus that touches
> it."
>
> **C12.F13.1 Superseded:** C14.4 SC-E as far as it makes another client's fullscreen enter/exit
> a non-client stimulus for a third element; INV-SCALE-5 "equals Phase B MaxGesture" for a held
> element.
>
> **C12.F13.2 What this layer does not change:** SC-E for a client's own geometry; P7; system
> changes; the order of hints before configures. Cost, accepted by this layer: a held element may
> keep a higher buffer scale than its remaining observers need (memory, never sharpness) until a
> stimulus touches it.
>
> Tests (T0): `a_client_cycling_fullscreen_does_not_churn_another_clients_hint`,
> `a_clients_presentation_holds_the_hints_of_the_elements_it_hides`,
> `a_held_window_committing_between_toggles_keeps_its_hint`,
> `a_passive_window_under_two_presentations_keeps_its_hint`,
> `a_revealed_window_is_raised_after_a_system_change_lowered_it`,
> `a_hold_ends_when_the_presentation_that_caused_it_ends`, `a_held_windows_own_growth_ends_its_hold`,
> `a_held_windows_own_fullscreen_ends_its_hold`, `p7_lowers_a_held_window`,
> `a_camera_drag_releases_only_the_holds_it_touches`, `an_output_connected_releases_every_hold`,
> `a_device_limit_report_releases_no_hold_now_or_later`,
> `the_end_of_a_presentation_raises_the_hint_of_a_revealed_element`; T1 (integration line)
> `a_hidden_windows_lowered_hint_releases_no_withheld_callback`; seeded controls "C12.F13: …".

## P-4. C12.F9 — withdrawal record

Dependants checked (EXIT_REVIEW_R2 §4, re-checked 2026-10-16): no invariant, formula, obligation,
checker rule, test or seeded control names C12.F9; the only texts are the deferral note and the
reserved number. CHECKPOINT governance line, replacing "C12.F9 (INV-ARCH-2 test-only allocator)
is deferred to the INC-6 exit":

> C12.F9 **withdrawn** (owner, `<DATE>`): the OB-42 counting allocator moved to the headless
> backend's tests (2026-10-15); INV-ARCH-2 is unchanged and Core's test exception is removed; the
> number C12.F9 stays reserved and unused. Correction of the owner decision of 2026-10-14, which
> approved the exception and deferred its layer.

FOUNDATION_INVARIANTS line 18 and FOUNDATION_ARCHITECTURE §C12.F10 ("C12.F9 is reserved for the
INC-6 exit"): append "(withdrawn `<DATE>`; the number stays unused)". UNSAFE_INVENTORY history:
"The 2026-10-14 decision awaits the owner's formal correction" → "was corrected by the owner on
`<DATE>` (C12.F9 withdrawn)". KNOW-DEC-005 line 33 and KNOW-ROAD-005: a dated note.

## P-5. OB-28 split → OB-28 + OB-53 (FOUNDATION_TEST_PLAN)

OB-28, **Item** becomes: "Client resizing in response to scale hints, for the stimuli INC-6
provides (the INC-6 part of the original OB-28): an adversarial self-resizing client under SC-E
after a non-client stimulus, under P7, and inside a continuous camera-pan epoch; cross-client
presentation churn (C12.F13)." **Pass criterion, destination, test class: unchanged.** History
gains: "**Split `<DATE>` (owner decision):** the union of OB-28 and OB-53 is the original;
nothing of its criterion is dropped."

New entry:

> ### OB-53
> - **Status:** OPEN
> - **Item:** Client resizing in response to scale hints, for the stimuli later increments
>   introduce (the remainder of the original OB-28): the viewport-fit feedback loop of EC-22
>   (INV-CAMERA-5) and discrete CameraZoom epochs (INC-10), WindowMove / WindowResize epochs
>   (INC-14), ShellStream epochs (INC-11).
> - **Owner:** core scale policy + the owning increment
> - **Evidence available:** MODEL EC-22, ER-5 R2/R3, ER2-2
> - **Pass criterion:** with an adversarial T1 client: hint changes ≤ (rungs crossed + 1 + one P7
>   per non-client stimulus) per epoch; the run ends quiescent; no camera mutation from client
>   geometry (INV-CAMERA-1); re-fit rate ≤ commit rate (EC-22)
> - **Destination:** T1
> - **May be closed by:** T1 production tests in INC-10, INC-14 and INC-11, each for its stimuli;
>   closes when all three parts are met
> - **Dependencies:** INC-10, INC-11, INC-14, OB-28
> - **Blocks:** the exits of INC-10 (fit loop, CameraZoom), INC-14 (WindowMove/Resize), INC-11
>   (ShellStream), each for its part
> - **Origin:** OB-28 split (owner decision `<DATE>`); the requirement: CP3A-4; C12.R1.3

Citations: FOUNDATION_IMPLEMENTATION_PLAN INC-10 (`Obligations: OB-14 …, OB-24 …`), INC-11
(`OB-35`) and INC-14 (`OB-34 …`) each gain ", OB-53 (its part)". FOUNDATION_INVARIANTS
INV-SCALE-4 and FOUNDATION_RISKS RC-2: `Obligations: OB-28, OB-53`. The sealed registry row of
OB-28 (FOUNDATION_ARCHITECTURE line 1079) is not edited (as for OB-52). `check-foundation-docs`:
OB-53 registered, destination T1.

## P-6. OB-42 — closure wording, evidence kept

Today: "**May be closed by:** T0 PRODUCT test in core". The test is no longer in Core: INV-ARCH-2
forbids `unsafe`, and so a counting global allocator, in every Core target, and the owner's
direction of 2026-10-15 moved it to `crates/infynitae-backend-headless/tests/`. What made "in
core" valuable — that only Core is measured — is kept by stating it as a requirement and
checking it mechanically.

FOUNDATION_TEST_PLAN OB-42, **Pass criterion** becomes:

> zero heap allocations per event after warm-up, every round, at 10, 100 and 1,000 elements, on
> each Core hot path INV-ALLOC-1 lists (input intake, pan, pointer routing, hit-testing,
> visibility, render-list construction) and on frame tick and commit (C14.3); counted by a
> test-only counting global allocator, per thread, around calls of Core's production
> caller-buffer entries only; frees are not counted

**May be closed by** becomes:

> T0 PRODUCT test of Core's entries. Its test binary is outside Core (INV-ARCH-2 forbids
> `unsafe` in every Core target; owner direction 2026-10-15): today
> `crates/infynitae-backend-headless/tests/t0_hot_path_allocations.rs`. Conditions, each checked:
> the file names no crate but `std` and `infynitae_core` (`verify-architecture`, with two seeded
> violations); production reaches only the measured caller-buffer entries (source rule
> `callers-use-caller-buffer-entries`, four seeded violations); seeded reintroductions of an
> allocation are detected (`verify-test-efficacy`)

History gains the closure line with the date, the commit and the measured result, and its scope:
steady state only — a batch that emits a scale hint or settles an epoch, a constraint change, a
binding's action and a button press still return a `Vec` (`FINAL_RESOLUTION_2026-10-15.md`).

## P-7. Mechanical steps after the decisions

1. Governance branch from the candidate: P-1 … P-6; `F_LAYERS` gains C12.F11, C12.F12, C12.F13
   with `@ ADOPTED <DATE>` origins; one drift fixture per layer (as 26/27 for C12.F3/F4) and one
   for OB-53 closed; FOUNDATION_INVARIANTS layer lines for INV-SCALE-2, INV-SCALE-5.
2. Code comments and seeded-control names: "REHEARSAL" / "AMENDMENT NOT ADOPTED" → the layer
   name.
3. CHECKPOINT entry; every gate (`tools/verify-development-environment`).
4. Independent review of that branch; the owner's INC-6 ratification; fast-forward `main` only
   then; push only with the owner's approval.
