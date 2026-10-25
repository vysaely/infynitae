# INC-7 — independent audit, adversarial coverage and exit-readiness, 2026-10-14

Status: **EXPERIMENTAL evidence. Not authority. INC-7 is not ratified and its exit is not
claimed. No obligation is closed by this file.**

Written by a second session working in its own worktree while the INC-7 line was being advanced
by another. The audit read the sealed text and the code and did not rely on the README beside
this file. Base audited: `aa9c497` at first, then `7aed2f0` and `782a3d3` as they appeared; every
statement below is about `782a3d3` plus the three commits of `continuation/inc-7-audit-r2`
unless it says otherwise.

## 1. Findings

Severity: **High** = wrong protocol state or lost input for a conforming client in ordinary use;
**Medium** = wrong behaviour in a sequence real clients produce, or a policy hole; **Low** =
deviation with no observed consequence; **Info** = dependency or note.

| # | Severity | Finding | State |
|---|---|---|---|
| A-1 | Medium | The size saved at a fullscreen grant is the element's geometry at that moment. After `unset_fullscreen` and before the client redraws, that geometry is still the fullscreen one, so a second grant saves the fullscreen size and the next exit offers it. The windowed size is lost | **defect; fixed** in `77cde7f` |
| A-2 | Medium | A popup whose parent is a popup is not constrained to the viewport: `place_popup` resolved the parent as an element root, failed, and used the positioner's unconstrained geometry. A submenu near an output edge is never slid or flipped | **defect; fixed** in `8db8080` |
| A-3 | Medium | INV-PRES-1 grants fullscreen "before W's first map" and lets "a newer grant replace an older one on the same output". Together: a toplevel that has never drawn, from any client, ends the focused element's fullscreen. The reading "the output is taken over at map" prevents a blank output but not the eviction | **sealed behaviour; owner decision** (§5) |
| A-4 | Medium | A buffer in a popup's initial commit was answered with a configure instead of a protocol error (W-SM-6, OB-18) | found here independently; **already fixed** by `782a3d3` with the same change. Confirmed by a second suite |
| A-5 | Low | `Core::true_position(0)`: the pointer event synthesised when a presentation starts or ends, and on output connect, reconfigure and disconnect, carries time 0. `wl_pointer.motion.time` is then not monotonic for the client. Read in the code (`protocol.rs` `presentation_changed`, `entry.rs` three sites); not observed on the wire, the probe does not record times. The three `entry.rs` sites are in ratified INC-2/INC-5 code | open; needs a time source Core does not have on this line (the session clock of C12.F4 would serve) |
| A-6 | Low | The pinned Smithay answers three client errors with other codes than xdg-shell names: `xdg_surface.not_constructed` for a buffer on an unconfigured surface (spec: `unconfigured_buffer`); `xdg_wm_base.invalid_surface_state` for an unknown acknowledged serial (spec: `xdg_surface.invalid_serial`); and **no error** for `get_popup` on a parent without a role (spec: `invalid_popup_parent`), which the adapter then survives by dismissing the popup | recorded; candidates for W-SM entries |
| A-7 | Info | An element hidden by a presentation is unobserved and gets no frame callback (kick and fallback are INC-8). A client under a fullscreen window that draws from callbacks stops until the presentation ends | dependency on INC-8; reproduced |
| A-8 | Info | Keyboard focus may rest on, or be moved to, an element a presentation hides (`set_focus` to the hidden window succeeds; the fullscreen element keeps the output and loses `activated`). Sealed: "focus loss does not end fullscreen". Whether focus may go to something invisible is a shell-policy question | owner / INC-11 |
| A-9 | Info | Two seeded controls of the INC-7 line are weaker than they read. (a) "a camera drag moves what a fullscreen output shows" seeds `view.v.x`; a pixel test passes with that seed, so the field does not reach the rendered frame, and only the T0 comparison of the `View` value catches it. (b) "a postponed placement survives the element" is MISSED on `782a3d3` in that line's own gate log and here | (a) a pixel-visible seed added here; (b) belongs to the line's author |
| A-10 | Info | Core now holds three per-element side tables for fullscreen (`premap_fullscreen`, `windowed_placement_pending`, and `restore_offers` added by A-1's fix), each purged at the same three points. One per-element record would remove the chance of purging two of three. Not changed: nothing is wrong today | suggestion for the exit review |

Audited with no finding: Core as the only authority over toplevel requests (the adapter's eight
handlers forward and answer nothing; every configure is rebuilt from Core); configure sequencing
(one per request, in request order, also within one dispatch); grant and decline; pre-map and
post-map lifecycle; exit by unset, unmap, destruction, replacement and output removal; output
association and removal, including the last output; camera independence; pointer routing through
the presentation view; cleanup after destruction and after a killed client; fractional and mixed
scales; damage (an unchanged presentation produces none).

## 2. Reproductions

All in `crates/infynitae-backend-headless/tests/`, a real client over the protocol.

| # | Test | On `782a3d3` | With the fix |
|---|---|---|---|
| A-1 | `t1_inc7_adversarial::re_entering_before_redrawing_windowed_keeps_the_windowed_size` | FAILS: last configure `(640, 480) [activated]`, expected `(200, 100)` | passes |
| A-1 | separate process: `fs_client --size 400x300 --script "500:fullscreen;1500:unfullscreen;1500:fullscreen;2500:unfullscreen;3500:exit"` against a headless compositor | build `6a126f0`: the last configure is `1280x800 ["activated"]` | this branch: `400x300 ["activated"]` |
| A-2 | `t1_inc7_adversarial::a_nested_popup_is_constrained_like_a_first_level_one` | FAILS: nested popup configured at `(90, 10)`, expected `(70, 10)`; the same geometry as a first-level popup is slid correctly | passes |
| A-3 | `t1_inc7_adversarial::as_sealed_a_pre_map_grant_replaces_a_presented_fullscreen` | passes: it asserts the sealed behaviour (W is told to leave; focus is still W's) | — |
| A-4 | `t1_inc7_misbehaving::a_popup_buffer_on_the_initial_commit` | passes on `782a3d3`; failed on `7aed2f0` (no error raised) | — |
| A-7 | `t1_inc7_adversarial::until_inc_8_a_window_hidden_by_a_presentation_gets_no_frame_callback` | passes: no callback in 30 frames, one at the first frame after the presentation ends | to be replaced by INC-8's test |

## 3. Tests added and the invariant each exercises

`t1_inc7_adversarial.rs` (18 tests; output pixels through the production render path):

| Test | Invariant |
|---|---|
| `re_entering_before_redrawing_windowed_keeps_the_windowed_size` | M9 "configure with the saved size" |
| `fifty_toggles_without_a_single_redraw` | INV-PRES-1 one configure per request; membership balanced; no anchor, camera or order change |
| `a_set_and_an_unset_in_one_batch_get_one_configure_each` | the same within one dispatch, in order |
| `destroyed_before_acknowledging_the_grant` | INV-STAGE-4 purge; presentation ends at destruction |
| `unmapped_before_acknowledging_the_grant` | M1 "fullscreen dropped"; re-show is an ordinary initial configure |
| `a_stale_acknowledgement_does_not_bring_fullscreen_back` | Core's decision is the state; an acknowledgement is not |
| `a_client_that_never_answers_the_grant` | presented as it is; the configure is not repeated; no damage; exit still works |
| `an_oversized_buffer_is_origin_aligned_and_cropped` | F-PRESENT-1, pixels and hit-test |
| `twenty_rounds_of_fullscreen_unmap_and_reshow` | no state outlives a mapping; INV-STAGE-5 |
| `a_hidden_window_gets_no_pointer_input` | C5.2 hit-testing through the presentation view; one true position at the start |
| `a_camera_pan_under_a_presentation` | INV-CAMERA-4; the hidden camera moves, no pixel and no configure |
| `a_window_mapped_under_a_presentation_stays_hidden` | exclusivity; a map grants nothing |
| `the_last_output_is_removed_under_a_presentation` | INV-OUTPUT-1, M9 row 6, M4 last output |
| `fullscreen_on_outputs_of_different_scales` | C5.2 size on a fractional output; M4 "one configure only for … this output" |
| `as_sealed_a_pre_map_grant_replaces_a_presented_fullscreen` | INV-PRES-1 as sealed (A-3) |
| `until_inc_8_a_window_hidden_by_a_presentation_gets_no_frame_callback` | M16 as it is before INC-8 (A-7) |
| `a_nested_popup_is_constrained_like_a_first_level_one` | M3 target; OB-1 |
| `a_menu_chain_of_a_fullscreen_window` | C5.2 popups in the presentation; INV-INPUT-10; focus loss keeps fullscreen |

`t1_inc7_misbehaving.rs` (6 tests, OB-18; a second client connected throughout must still map,
be configured and be focused after each offence): buffer on a toplevel's initial commit; buffer
before a toplevel's acknowledgement; the same two for a popup; an acknowledgement of a serial
never sent, on a fullscreen window; a grabbing popup on a parent without a role.

T0: `protocol::tests::the_saved_size_is_the_last_windowed_size`.

**Seeded controls: 24 added, all detected** (gate run on `904fcde`). Each of the 24 tests above
that guards a behaviour has at least one seed, except `a_stale_acknowledgement…`: I found no
one-line defect that expresses "the adapter trusts an acknowledgement", so it has none and its
value is unproven by seeding.

Not covered here, and why: a client **disconnect** in the middle of a configure is exercised
through destruction of its objects in the lockstep harness and through `SIGKILL` of a separate
process (§6), not through a socket closed inside the harness; real toolkits (INC-15).

## 4. Exit-readiness against the sealed INC-7 text

Classification of evidence only. Closing an obligation, and the exit review, are the owner's.

| Sealed item | Class | Evidence / gap |
|---|---|---|
| M2/M9: every request answered with exactly one configure | VERIFIED | T0 request table (8 requests × 4 states); T1 `tst22…`, `t1_fullscreen`, fifty toggles, one-batch |
| Fullscreen presentation (F-PRESENT-1) | VERIFIED | T0 hand-derived; T1 pixels: centred, exact, oversized, fractional output |
| Maximize declined; minimize ignored; capabilities truthful | VERIFIED | T0, T1, seeded |
| W-SM-13 overrides (the adapter answers nothing itself) | VERIFIED | seeded controls on three handlers |
| INV-PROTO-1 incl. `configure_bounds` | VERIFIED | C12.F3 adopted on `main` (`ccb82c5`); `t1_protocol_bounds` |
| INV-CAMERA-4 | VERIFIED | T0; T1 pan under a presentation with pixels and configure count |
| INV-PRES-1, "saved size" | PARTIALLY VERIFIED | correct only with `77cde7f` (A-1), which is on a continuation branch, not on the INC-7 line |
| INV-PRES-1, acceptance "no output seized while the user is elsewhere" (OB-22 T1 part) | PARTIALLY VERIFIED | unfocused requests are declined (tested); a pre-map newcomer evicts a presented element (A-3): sealed, but against the stated rationale |
| Default placement of a window that starts fullscreen | BLOCKED | implemented on the line (`2044967`) against the letter of INV-STAGE-1 / C12.F1; the amendment is prepared by the line's author and not adopted |
| Popup robustness W-SM-6 / W-SM-7 (OB-18) | VERIFIED | `t1_popup_policy` (line) and `t1_inc7_misbehaving` (here, with a bystander); A-6 records the codes |
| Nested popup chain parity (OB-1; exit test) | PARTIALLY VERIFIED | order, positions and per-pixel hit-test: `ob1_nested_popup_chain_matches_what_is_presented` (line). Constraint of a nested popup is correct only with `8db8080` (A-2) |
| TST-22 xmaximize on the production path (OB-32; exit test) | VERIFIED | `t1_toplevel_requests`, `t1_fullscreen` |
| Negative controls: record-only / deferral must fail | VERIFIED | seeded: "the focused element is not granted fullscreen (slice 1's deferral)", "a move request is answered by the adapter itself", both detected |
| Stop condition: any unanswered mandatory request | none found | |
| OB-22 T3/T5 part (games, video, X11) | NOT APPLICABLE to the INC-7 exit | the obligation's record says it blocks release, at INC-15/17 |
| Seeded-defect gate green on the line | BLOCKED | one control MISSED on `782a3d3` (A-9 b) |
| Readings of the sealed text | PARTIALLY VERIFIED | the line records owner decisions of 2026-10-14 for items 1–8; I did not find them written into a sealed layer |
| Formal exit review, human review | NOT DONE | owner |

Not INC-7's, but it will meet INC-7 at integration: the INC-6 × INC-7 glue (a presented element
contributes its output's scale; a presentation change re-evaluates the scale policy) exists only
on the application-validation branch and is provisional.

## 5. Decisions this audit needs from the owner

1. **A-3, pre-map eviction.** Options: (a) keep as sealed; (b) a pre-map grant on an output that
   already presents another element is stored but replaces nothing until the newcomer maps;
   (c) such a request is declined in the initial configure unless the output is free. (b) and
   (c) change INV-PRES-1 and need a registry layer. My recommendation is (b): it keeps the
   satellite's pre-map path (EC-13) and removes the case where a client that never draws ends
   someone else's fullscreen.
2. **A-1 and A-2 fixes.** Whether `77cde7f` and `8db8080` go onto the INC-7 line. A-1 adds one
   piece of Core state (the size on offer); if that is unwelcome, the alternative is to save
   the size in the adapter's acknowledged state, which I advise against (a second authority).
3. **A-5.** Whether synthesised pointer events should carry the session clock's time once
   C12.F4's clock is on the line, or stay at 0.
4. **A-6.** Whether the three Smithay error-code deviations get W-SM entries.
5. **A-8.** Whether focus may be given to an element a presentation hides.

## 6. Separate-process runs

Headless compositor binary from this branch, private runtime directory, no session bus, no host
display, pure Wayland client (`fs_client`, built on the application-validation branch). Logs
under `~/.cache/infynitae-integration/cont/real/`.

| Run | Result |
|---|---|
| `a1-fixed`: toggle twice inside one dispatch | configures `1280x800 [fullscreen, activated]`, `400x300 [activated]`, `1280x800 [fullscreen, activated]`, `400x300 [activated]`; client exit 0, compositor exit 0, runtime directory empty |
| `a1-before`: the same against the build of `6a126f0` | last configure `1280x800 [activated]`: the defect, seen by a real client |
| `kill-fs`: the client is sent `SIGKILL` 1 s after it went fullscreen | compositor keeps running to its time limit and exits cleanly; no process and no file left |

Not run: any toolkit application. KCalc was run by the line's author; nothing here adds to that.

## 7. Gates

On `904fcde` (`continuation/inc-7-audit-r2`), isolated environment:

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | pass |
| `cargo test --workspace --locked --offline` | 336 passed, 0 failed |
| `tools/verify-architecture` | pass |
| `tools/check-foundation-docs`, `--self-test` | pass, pass |
| `tools/verify-test-efficacy` | **FAIL (1)**: 217 of 218 detected. The miss is the line's own control "a postponed placement survives the element", missed identically on `782a3d3` in the line's log. All 24 controls added here are detected |

An earlier run on `a89448a` (the same work on the older base `aa9c497`): 317 tests, all other
gates pass, 200 of 201 controls; the miss was my pixel test against the weak seed of A-9 (a),
which is why that seed was replaced.

`tools/verify-development-environment` was not run.
