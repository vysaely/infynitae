# C12.F13 — the source of a presentation change in the scale policy — PROPOSAL

Status: **PROPOSAL, NOT ADOPTED** (2026-10-15). Evidence, not authority; no living document is
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
> its hint against decreases until a non-client stimulus that is not such a change (an output
> change, a camera action, its own fullscreen enter/exit) evaluates it, or until an evaluation
> finds its requirement back at or above its hint. P7 applies regardless.
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
- Any camera drag releases every hold: its first step is a full non-client pass and its release
  settles, even on an output that never observed the held element (independent re-review). This
  follows from "a camera action" above; the owner may prefer to limit release to stimuli that touch
  the element.

## What it does not change

SC-E for a client's own geometry; P7; the epochs of non-subjects; system-initiated changes; the
order of hints before configures (resolution branch).
