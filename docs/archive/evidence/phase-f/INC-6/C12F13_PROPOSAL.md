# C12.F13 — the source of a presentation change in the scale policy — PROPOSAL

Status: **PROPOSAL, NOT ADOPTED** (2026-10-15; text corrected to the candidate's code 2026-10-16). Evidence, not authority; no living document is
changed by it. Owner direction of 2026-10-15, item 1: "Client-requested fullscreen must not
indiscriminately release unrelated clients' held scale hints. Separate the source of a request
from the compositor's resulting authoritative state change. Preserve necessary scale reevaluation
without enabling cross-client churn." Rehearsed on `rehearsal/inc-6-scale-amendments-2026-10-15`
(with C12.F11 and C12.F12); never merge before adoption.

## Why it needs a layer (the contradictions)

A first implementation on the resolution branch (`28421bc`) treated it as an interpretation. The
independent review of 2026-10-15 showed it is not:

- **C14.4 (SC-E)** lists "fullscreen enter/exit" among the non-client stimuli and defines a client
  response as a commit that changes the client's own geometry. Another client's fullscreen request
  is neither, so classifying its effect on a third element as a client response is a new rule.
- **INV-SCALE-5**: "At zoom 1 with no zoom interaction … the policy equals Phase B MaxGesture."
  Holding a hidden element's hint above the scale of the outputs that still observe it departs
  from MaxGesture; INV-SCALE-5 allows holding only for an unobserved element.
- **M9** lists "settlement" among the effects of a grant without saying whose epoch it settles.

The canonical behaviour (restored on the resolution branch at `546bf83`): a presentation that
starts or ends is a non-client stimulus for every element it touches. Measured: a client that
never holds focus cycling create / pre-map grant / map / destroy moves another window's hint twice
per cycle (40 changes in 20 cycles: the review's probe on `3666286`); with a further 20 focused
toggles by another client, 80 (the implementer's test, on `546bf83`, the same as on `3666286`).

## Proposed canonical text

C14.4, after the list of non-client stimuli:

> Fullscreen enter/exit is a non-client stimulus for the element whose presentation starts or ends
> (its subject); its open epoch settles (M9 "settlement"). For every other element whose observers
> change only because a client's request, commit or destruction started or ended a presentation,
> the change may raise its hint (as a non-client stimulus) but never lowers it: the element holds
> its hint against decreases until a non-client stimulus that is not such a change **and that
> touches the element** evaluates it (an output added, removed or reconfigured touches every
> element (a reported device limit or a configuration change touches none: P7 applies at once;
> second correction of 2026-10-16, with a code fix); a camera action touches the elements that an
> output bound to the moved camera
> observed before it or observes after it; its own fullscreen enter/exit touches it), or until
> an evaluation finds its
> requirement back at or above its hint. A settlement does not lower a held element its
> stimulus never touched. P7 applies regardless.
> A presentation the system ends or starts (output added, removed, reconfigured) is a non-client
> stimulus for every element.

INV-SCALE-5 gains: "… equals Phase B MaxGesture, except that an element held under C14.4 (another
client's presentation) keeps a higher hint until its next non-client stimulus."

## Consequences

- Under client presentation changes an unrelated element's hint is non-decreasing, so it changes at
  most |L(E)| times until a non-client stimulus releases the hold: no oscillation, whatever the
  presenting client does, and whatever the held element's own commits do (its own commits are
  client responses, and the hold blocks their decreases too).
- A revealed element whose requirement rises is raised in that batch, so it is never left
  under-resolved by another client's presentation (the regression of the first implementation).
- Cost: an element hidden by another client's presentation may keep a higher buffer scale than its
  remaining observers need (memory, never sharpness) until its next non-client stimulus.
- Only a stimulus that touches the held element releases its hold. **Correction of 2026-10-16
  (text brought to the code of `4c81a1c`, no behaviour change):** this bullet said "Any camera
  drag releases every hold: its first step is a full non-client pass and its release settles,
  even on an output that never observed the held element … the owner may prefer to limit release
  to stimuli that touch the element". `4c81a1c` implemented that narrower option: a camera
  action releases the holds of the elements on the moved output's membership rows only, and an
  output change touches everything. Tests (T0, `infynitae-core`):
  `a_camera_drag_releases_only_the_holds_it_touches`,
  `a_revealed_window_is_raised_after_a_system_change_lowered_it`,
  `an_output_connected_releases_every_hold`, `a_hold_dies_with_its_element`; seeded controls
  "C12.F13: a camera action releases every hold, touched or not", "… an epoch's settlement
  lowers a held element it never touched", "… an output change does not touch every element".

## What it does not change

SC-E for a client's own geometry; P7; the epochs of non-subjects; system-initiated changes; the
order of hints before configures (resolution branch).
