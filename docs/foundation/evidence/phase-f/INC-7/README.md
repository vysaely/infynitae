# INC-7 — toplevel request policy: experimental, slices 1 to 3

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

## Fullscreen lifecycle decisions (owner, 2026-10-14) and where each is tested

| Decision | Implementation | Test |
|---|---|---|
| 1. Unmap invalidates the fullscreen state and every pending piece of it | the state, a recorded pre-map request and a postponed placement are dropped together | T0 `fullscreen_ends_cleanly…`, `placement_after_fullscreen…`; T1 `unmap_remap_and_destruction…` |
| 2. Output takeover begins at map, not at grant | the presentation view is used only while the element is mapped | T0 `a_fullscreen_request_before_the_first_map…`; T1 `a_client_that_asks_before_mapping…` |
| 3. A remap is not a first map for focus eligibility | the pre-map grant needs "never mapped before" | T0, same test; seeded control |
| 4. Fractional output dimensions use the established rounding | inward (R8): the configured size is the floor of the logical extent | T0 `the_presentation_view_is_f_present_1`; seeded control |
| 5. Hidden camera movement is permitted | a camera drag on a fullscreen output moves the camera and not the picture; the client receives no pointer event during it; a pointer lock stays active; a popup grab is dismissed first, as for any drag | T0 `a_presentation_is_independent_of_the_camera`; T1 `a_hidden_camera_drag_respects…`, `a_popup_chain_on_a_fullscreen_window_is_dismissed_by_a_camera_drag` |
| 6. Initial fullscreen does not consume default placement | a map in fullscreen leaves the anchor provisional and the opportunity open | T0 `a_window_that_starts_fullscreen_is_placed_at_its_first_windowed_size` |
| 7. The first windowed presentation establishes the anchor | at the first windowed map, or at the first commit after fullscreen ended whose geometry differs from the one it had then | same; T1 `a_client_that_asks_before_mapping…` (pixels) |
| 8. An existing windowed anchor is restored | fullscreen never writes an anchor, and a placed element is never placed again | T0 `placement_after_fullscreen_covers_every_exit…` |

**Sealed wording affected by 6 and 7 (not changed; amendment prepared for the owner):**
INV-STAGE-1 (C12.F1) says default placement is "resolved once, at the element's first map", and
F-PLACE-1 names "the window-geometry size at the element's first map". Decisions 6 and 7 make
that "first windowed presentation". The behaviour is on this experimental branch only.

One judgement inside decision 7: "the first windowed size" is recognised as the first commit
whose geometry differs from the fullscreen one. A client's frames still in flight at the
fullscreen size therefore do not trigger placement; a client that never changes size is never
placed and keeps its provisional anchor.

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

## Slice 3: popup policy

Sealed popup items of INC-7: "popup robustness (W-SM-6/7)"; OB-18 (its W-SM-7 half was met at
INC-4, its W-SM-6 popup half was open); OB-1 (nested popup chains: order and positions against
Smithay's render, hit-test against what is presented, 0 mismatches); exit test "nested popup
chain parity". Popups stay what the sealed model makes them: surfaces of their toplevel's tree,
never Stage elements.

| Item | Before | Now | Test |
|---|---|---|---|
| OB-18, buffer in a popup's **initial** commit | **accepted**: the offending commit was answered with a configure | protocol error, client disconnected; the check runs before the configure is sent | T1 `a_buffer_in_a_popups_initial_commit_is_a_protocol_error` |
| OB-18, buffer before the configure is acknowledged | protocol error (present since INC-4), untested | unchanged, tested | T1 `a_popup_buffer_before_the_configure_is_acknowledged…` |
| The same inside a grabbing chain | untested | the chain's grabs go with the client; no element, focus or membership row is left | T1 `an_offending_nested_popup_takes_its_chain…` |
| OB-1, nested chains | one level verified | three levels, a sibling popup and a subsurface in a popup: snapshot equals hand-derived truth; order equals Smithay's; hit-test equals what is presented on every pixel, at scale 1 and 1.25 | T1 `ob1_nested_popup_chain_matches_what_is_presented` |
| Popups over a fullscreen window | — | survive the grant with their grab; presented with the window; constrained to the fullscreen output, including when another output's camera also shows the window; dismissed by a camera drag topmost first | T1 `a_popup_survives_the_grant…`, `a_popup_chain_on_a_fullscreen_window…`; T0 output-choice test |

Found while testing, not changed (each needs an owner ruling or a quirk record):

- **Smithay's error code.** For a buffer on an unconfigured surface Smithay 0.7.0 posts
  `xdg_surface` error 1 (`not_constructed`, "Surface has not been configured yet"); xdg-shell
  names error 3 (`unconfigured_buffer`) for it. The client is disconnected either way. The tests
  accept both codes.
- **A dead client's popup pointer grab is removed at the next pointer event, not at the client's
  death.** The keyboard grab goes at once. Nothing is routed to the dead client meanwhile; the
  test sends one motion and one click and then requires no grab.
- **Sibling popups of one parent are stacked oldest in front.** That is Smithay's tree order and
  the render list follows it (the parity test includes a sibling). Overlapping siblings are
  unusual; a later popup drawn under an earlier one would look wrong.

## Deferred inside INC-7

- The T3/T5 part of OB-22 (games, video players, X11 through the satellite).
- OB-32, OB-18 and OB-1: their criteria are met on this branch by the tests named above. They
  are **not closed here**: the branch is experimental and unratified, and closing an obligation
  belongs to the increment's exit review.

## Found, not changed

- Smithay sends `xdg_toplevel.configure_bounds(0, 0)` with the first configure to every client
  that binds `xdg_wm_base` v4 or later. Zero means "no bounds". INV-PROTO-1 says no
  `configure_bounds` is sent in Foundation 01; with the pinned Smithay that cannot be met to the
  letter. The test `the_announced_capabilities_are_what_the_policy_grants` records it and fails
  if a non-zero bound is ever proposed. This needs an owner ruling (a W-SM entry, or a wording
  change).
- Whether a given toolkit hides a control when its capability is absent is the toolkit's choice.

## Tests

T0 (Core): the request table; stale element; capabilities equal what the policy grants; grant,
presentation and release with hand-derived values; pre-map; replacement and focus loss; unmap,
destruction and output changes; output choice and popup constraint with two outputs;
F-PRESENT-1 on fractional outputs; independence from the camera; placement after fullscreen
(two tests).
T1 (a real client over the protocol): `t1_toplevel_requests` (6), `t1_fullscreen` (10, with the
output's pixels), `t1_popup_policy` (3), `t1_parity::ob1_…` (1), `t1_protocol_bounds` (1).
Seeded controls: 35.
