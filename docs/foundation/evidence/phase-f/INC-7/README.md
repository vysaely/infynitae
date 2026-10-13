# INC-7 — toplevel request policy: experimental entry, slice 1

Status: **EXPERIMENTAL. Entry authorized by the owner 2026-10-13. Not ratified; INC-7's exit
tests are not met and its exit is not claimed.** This file is evidence, not authority.

## Sealed scope read before any code

| | |
|---|---|
| Objective | M2/M9: answer every request with exactly one configure; fullscreen presentation (F-PRESENT-1); maximize declined; W-SM-13 overrides; popup robustness (W-SM-6/7) |
| Invariants | INV-PRES-1, INV-PROTO-1, INV-CAMERA-4 |
| Object | OBJ-19 (FullscreenState + OutputPresentation) |
| Obligations | OB-32, OB-22 (T1 part), OB-18, OB-1 (nested popups) |
| Exit tests | TST-22 xmaximize on the production path; nested popup chain parity |
| Negative controls | record-only / deferral (0 answers) must fail |
| Entry condition | INC-4 exit (met) |
| Not INC-7 | interactive move and resize (INC-14, gated by OB-34) |

## What slice 1 contains

One decision point in Core, `Core::toplevel_request`, with the policy table in
`Core::toplevel_policy`. The adapter overrides all eight `XdgShellHandler` request methods and
forwards each to it; it answers none itself. `Core::toplevel_capabilities` is the only source of
what is announced to clients. A request writes nothing: there is no stored request state, so a
client cannot create a second authority over geometry, focus, lifecycle or a camera.

## Per capability

`maximize`, `fullscreen`, `minimize` and `window_menu` are the four `wm_capabilities`. `move` and
`resize` have no capability bit.

| Capability | Protocol requirement | Implemented | Announced | Request handling | Configure and ack | Stage / camera / lifecycle | Tests and client-visible behaviour |
|---|---|---|---|---|---|---|---|
| **Maximize** | the compositor must answer `set_maximized` / `unset_maximized` with a configure; it need not grant | declined by sealed policy (INV-PROTO-1: never granted, never a camera or Stage path) | **no** | Core: `Declined` when a configure sequence exists; `Ignored` before the initial configure and while unmapped | exactly one configure, no `maximized`, size 0×0; the client acks as usual | none | T0 table; T1 `tst22…`, pre-configure and unmapped cases, 300-request stream. KCalc: see the checkpoint |
| **Fullscreen** | every `set_fullscreen` / `unset_fullscreen` answered with a configure | **answer only.** INV-PRES-1 grants it to the focused element and before the first map; that needs the output presentation (M9, F-PRESENT-1, OBJ-19), which is **not built** | **no** (it would be a control that does nothing) | Core: `Declined` (focused or not); `Ignored` pre-configure and unmapped | exactly one configure without `fullscreen`, size 0×0 | none | same tests. **Deviation from INV-PRES-1 while deferred:** the focused and pre-map requests are declined instead of granted |
| **Minimize** | no answer required | ignored by sealed policy (INV-PROTO-1) | **no** | Core: `Ignored` | none | none | T0, T1 |
| **Window menu** | no answer required | no window menu exists; not in Foundation 01 | **no** | Core: `Ignored` | none | none | T0, T1 |
| Move (`xdg_toplevel.move`) | no answer required; starts an interactive move | **not implemented: INC-14** | no capability bit | Core: `Ignored`, explicitly | none | none; the anchor is unchanged | T0, T1 |
| Resize (`xdg_toplevel.resize`) | no answer required; starts an interactive resize | **not implemented: INC-14** | no capability bit | Core: `Ignored`, explicitly | none; no configure ever proposes a size | none | T0, T1 |
| `activated` | set exactly while the element holds ActiveElementFocus | yes (INC-4) | — | focus transitions | one configure per transition | — | INC-4 tests; here: declined answers carry `activated` exactly when focused |

## Deferred inside INC-7

- **Granted fullscreen** (M9 rows 1, 3, 4, 6; F-PRESENT-1; OBJ-19; pre-map grant in the initial
  configure; saved-size configure on exit; other elements leaving the output). This is the larger
  half of the increment.
- **Popup robustness** (W-SM-6 popup part, OB-18) and **nested popup parity** (OB-1).
- OB-32 is therefore **not** closed: its criterion includes the fullscreen policy state.

Managed borderless fullscreen (the intended long-term user experience) is a compositor policy
over windows and cameras. It is not protocol fullscreen and nothing here implements or
prejudges it.

## Found, not changed

- Smithay sends `xdg_toplevel.configure_bounds(0, 0)` with the first configure to every client
  that binds `xdg_wm_base` v4 or later. Zero means "no bounds". INV-PROTO-1 says no
  `configure_bounds` is sent in Foundation 01; with the pinned Smithay that cannot be met to the
  letter. The test `the_announced_capabilities_are_what_the_policy_grants` records it and fails
  if a non-zero bound is ever proposed. This needs an owner ruling (a W-SM entry, or a wording
  change).
- Whether a given toolkit hides a control when its capability is absent is the toolkit's choice.

## Tests

T0: `toplevel_requests_are_answered_by_policy_and_change_nothing` (8 requests × 4 lifecycle
states), `a_toplevel_request_on_a_destroyed_element_is_stale`,
`advertised_capabilities_are_exactly_what_the_policy_grants`.
T1 (`t1_toplevel_requests`, a real client over the protocol): 6 tests.
Seeded controls: 8, including TST-22's "0 answers" control and Smithay's default handler.
