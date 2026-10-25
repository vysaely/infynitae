# INC-11 — implementation record (2026-10-20)

Status: **IMPLEMENTED on branch `feature/inc11-shell-ipc-2026-10-20` (from
`integration/inc12-on-main-2026-10-20`); NOT RATIFIED; not `main`, not pushed.** Exit evidence
below; its acceptance (change control) and INC-11's ratification are the owner's.

## Commits

`17b6b1b` Core: ShellCommand and `shell_command`, shell streams, instance per run, metadata ·
`1df75bd` `infynitae-ipc`, the loop extension, the binary's socket, tests, controls · this record.

## Exit tests (FOUNDATION_IMPLEMENTATION_PLAN INC-11) → evidence

| Exit item | Evidence |
|---|---|
| stale-ID T1 | `crates/infynitae-ipc/tests/t1_shell.rs::stale_references_fail_and_never_alias`; T0 `shell_tests::stale_references_fail_and_never_alias`; across restarts `crates/infynitae/tests/shell.rs` (a new run is a new instance) |
| reconnect | `t1_shell::a_closed_connection_ends_its_stream_and_a_reconnect_resyncs` |
| command atomicity | `t1_shell::commands_run_in_order_and_failures_change_nothing`; T0 `navigate_outside_the_canvas_or_with_non_finite_values_changes_nothing`, `shell_commands_are_refused_while_the_output_presents` |
| shell stream epoch case (TST-23/24 on the production path) | `crates/infynitae-backend-headless/tests/t1_shell_stream_adversary.rs` (production Core + adapter + headless lockstep with a real Wayland client); T0 `a_shell_stream_is_one_epoch_until_its_end` |
| OB-35 (blocks INC-11's exit) | `t1_shell::attention_marks_end_with_unmap_and_destroy_and_the_shell_is_told` |
| OB-53 INC-11 part | `t1_shell_stream_adversary` (hint changes exactly 240 → 184 (P7) → 120 for 20 alternating steps; quiescent; no camera mutation) |
| Negative control: a raw-state command does not exist | by construction: `ShellCommand` has no raw write, `infynitae-ipc` reaches Core only through `shell_command` and public readers; source rules unchanged and passing |
| Negative control: shell stream without epoch (churn) | seeded control "INC-11 / INV-SCALE-3: a shell stream is not one epoch"; `the_same_steps_as_atomic_commands_make_the_hint_follow_them` (30 changes) |
| port binding | `verify-architecture` runtime check `shell_command_refuses_a_foreign_port` (was DEFERRED until INC-11) |
| channel (C7.1) | `crates/infynitae/tests/shell.rs`: 0700/0600, same-user admission, a live socket left alone, removed at shutdown; `shutdown.rs`: no socket left after a signal during start-up |

13 seeded controls ("INC-11 …" in `tools/verify-test-efficacy`), each DETECTED in a subset run.

## Findings during implementation [F]

1. **Instance aliasing across restarts (fixed):** `InstanceId` was the per-process Core counter,
   1 in every run, so a shell's `(1, n)` named a new element n after a restart (INV-STAGE-3).
2. **A start-up signal left the shell socket (fixed):** the socket was first made before the
   session handled SIGTERM/SIGINT; it is now made by the backend after `Session::new`.
3. **The single-writer rule (fixed):** the first `MoveElement` called `stage.place` from `entry`;
   `verify-architecture`'s source rule caught it; the write is in `protocol::place_by_shell`.

## Limits

- **Refresh cost:** with a subscriber, the server builds a derived copy after every loop dispatch
  (O(n) allocation outside Core, O(n log n) diff). It isn't measured at 1,000 windows.
- **Shell streams:** there is one stream per compositor. Two shells streaming at once share it.
- **Unvalidated values:** the protocol checks finiteness and the Canvas bound but no other range,
  for example a huge `Delta` on both axes. Core rejects anything beyond the stored Canvas.
- **D4 / OB-24:** decided, but not applied. Applying it needs U-8's checker change.
