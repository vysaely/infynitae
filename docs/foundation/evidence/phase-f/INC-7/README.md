# INC-7 — toplevel request policy: experimental, slices 1 and 2

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

## What exists (slices 1 and 2)

One decision point in Core, `Core::toplevel_request`. The adapter overrides all eight
`XdgShellHandler` request methods and forwards each to it; it answers none itself and grants
nothing. `Core::toplevel_capabilities` is the only source of what is announced to clients.

Slice 2 adds granted protocol fullscreen as sealed in C5.2, M9 and F-PRESENT-1:

- **State:** `Core.fullscreen: Vec<FullscreenState { element, output, saved }>` (OBJ-19), at most
  one per output and per element. It is the only fullscreen state in the compositor. The
  adapter builds every `xdg_toplevel.configure` from `Core::toplevel_configure_state` and writes
  every field each time, so Smithay's pending state never carries a stale flag or size.
- **Decision:** granted when the element holds ActiveElementFocus, or before its first map;
  otherwise declined at once with nothing stored. A request before the initial commit is
  recorded and decided in the initial configure. Exactly one configure answers every request.
- **Output choice:** the output the client named; else the element's frame-clock output; else
  the output under the pointer; else the first output.
- **Presentation:** while the fullscreen element is mapped, `Core::view(O)` is the presentation
  view (zoom 1; `v_P = anchor − max(0, (view_O/scale_O − size)/2)`), marked exclusive, so the
  render list, hit-testing and membership of O contain that element's tree alone. The camera of
  O is not read or written. Other outputs keep showing the element through their cameras at its
  real size.
- **Exit:** `unset_fullscreen` (one configure offering the saved size), unmap and destruction
  (state dropped, no configure), a newer grant on the same output (the older element gets its
  configure), output removal (configure with the saved size before the output goes). An output
  reconfiguration re-sends one configure with the new size.
- **Unchanged by any of it:** every anchor, every camera, the focus, the z-order.
- **Idle:** nothing is timed. A presentation changes membership and drops one cached frame
  when it starts, ends, or the element's geometry changes; an unchanged fullscreen scene
  produces no damage.

Not this: managed borderless fullscreen (a future compositor policy over windows and cameras),
direct scanout, or any output-level optimisation. None is implemented or prejudged.

## Readings of the sealed text made in slice 2 (for owner confirmation)

| Sealed text | What was implemented | Why |
|---|---|---|
| M9: "`unset_fullscreen` / unmap → configure with the saved size" | on **unmap** the state is dropped and **no configure** is sent | C5.2: "the spec discards client state on unmap … on remap the client must request again"; an unmapped xdg_toplevel has no configure sequence until its next initial commit. On `unset_fullscreen` the configure is sent as written |
| INV-PRES-1: "The presentation starts at the grant decision" | the state exists from the grant; the output is taken over when the element is **mapped** | before its first buffer there is nothing of the element to present; taking the output then would blank it for a client that never draws |
| INV-PRES-1: "or before W's first map" | literal: a re-show after an unmap is not a first map and needs the focus | wording |
| C5.2: "size = O's logical size at zoom 1" | a fractional logical size is rounded **down** | the surface is then never larger than the output; F-PRESENT-1 centres the remainder (under one pixel) |
| C5.2 (fit meanwhile changes the camera, "shown when the presentation ends") | a camera drag on a fullscreen output is not blocked: it moves the hidden camera | same rule applied to the pan; whether the binding should be inert there is a UX question |

## Per capability

`maximize`, `fullscreen`, `minimize` and `window_menu` are the four `wm_capabilities`. `move` and
`resize` have no capability bit.

| Capability | Protocol requirement | Implemented | Announced | Request handling | Configure and ack | Stage / camera / lifecycle | Tests and client-visible behaviour |
|---|---|---|---|---|---|---|---|
| **Maximize** | the compositor must answer `set_maximized` / `unset_maximized` with a configure; it need not grant | declined by sealed policy (INV-PROTO-1: never granted, never a camera or Stage path) | **no** | Core: `Declined` when a configure sequence exists; `Ignored` before the initial configure and while unmapped | exactly one configure, no `maximized`, size 0×0; the client acks as usual | none | T0 table; T1 `tst22…`, pre-configure and unmapped cases, 300-request stream. KCalc: see the checkpoint |
| **Fullscreen** | every `set_fullscreen` / `unset_fullscreen` answered with a configure; if granted the surface is shown alone on one output, centred if smaller | **yes** (slice 2) | **yes** | Core: `Granted` (focused, or before the first map), `Released`, `Declined` (not focused), `Recorded` (before the initial commit), `Ignored` (unmapped) | exactly one configure: `[fullscreen]` + the output's logical size; on release the saved size once | anchor, cameras, focus unchanged; the output's view becomes the presentation view; other elements leave that output | T0: 9 tests; T1 `t1_fullscreen`: 6 tests with pixels; `t1_toplevel_requests` |
| **Minimize** | no answer required | ignored by sealed policy (INV-PROTO-1) | **no** | Core: `Ignored` | none | none | T0, T1 |
| **Window menu** | no answer required | no window menu exists; not in Foundation 01 | **no** | Core: `Ignored` | none | none | T0, T1 |
| Move (`xdg_toplevel.move`) | no answer required; starts an interactive move | **not implemented: INC-14** | no capability bit | Core: `Ignored`, explicitly | none | none; the anchor is unchanged | T0, T1 |
| Resize (`xdg_toplevel.resize`) | no answer required; starts an interactive resize | **not implemented: INC-14** | no capability bit | Core: `Ignored`, explicitly | none; no configure ever proposes a size | none | T0, T1 |
| `activated` | set exactly while the element holds ActiveElementFocus | yes (INC-4) | — | focus transitions | one configure per transition | — | INC-4 tests; here: declined answers carry `activated` exactly when focused |

## Deferred inside INC-7

- **Popup robustness** (W-SM-6 popup part, OB-18) and **nested popup parity** (OB-1).
- The T3/T5 part of OB-22 (games, video players, X11 through the satellite).
- OB-32 is **not closed here**: its criterion is met on this branch by the T1 tests
  (`tst22…`, `t1_fullscreen`), but the branch is experimental and unratified, and closing an
  obligation is the owner's act.

## Found, not changed

- Smithay sends `xdg_toplevel.configure_bounds(0, 0)` with the first configure to every client
  that binds `xdg_wm_base` v4 or later. Zero means "no bounds". INV-PROTO-1 says no
  `configure_bounds` is sent in Foundation 01; with the pinned Smithay that cannot be met to the
  letter. The test `the_announced_capabilities_are_what_the_policy_grants` records it and fails
  if a non-zero bound is ever proposed. This needs an owner ruling (a W-SM entry, or a wording
  change).
- Whether a given toolkit hides a control when its capability is absent is the toolkit's choice.

## Tests

T0 (Core): the request table (8 requests × 4 lifecycle states); stale element; capabilities
equal what the policy grants; grant, presentation and release with hand-derived values; pre-map;
replacement and focus loss; unmap, destruction and output changes; output choice; F-PRESENT-1 on
fractional outputs; independence from the camera.
T1 (a real client over the protocol): `t1_toplevel_requests` (6) and `t1_fullscreen` (6, each
checking the client's configures, its enter/leave log and the output's pixels).
Seeded controls: 26.
