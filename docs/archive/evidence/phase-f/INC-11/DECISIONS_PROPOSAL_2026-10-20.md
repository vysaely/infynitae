# INC-11 (shell IPC) — decisions needed before implementation (2026-10-20)

Labels: [F] verified · [I] interpretation · [R] recommendation · [O] owner decision.
**Status: PROPOSAL.** Nothing here is decided or adopted; nothing is implemented.

**Spec:**
- FOUNDATION_ARCHITECTURE C7.1–C7.4;
- FOUNDATION_IMPLEMENTATION_PLAN INC-11;
- INV-SHELL-1/2, INV-SCALE-3 (ShellStream), INV-STAGE-3, INV-INPUT-9;
- OBJ-04, OBJ-31;
- OB-35, OB-53 (INC-11 part);
- ADR-11 (amended 1R: shell navigation is atomic, no animation in Foundation 01).

**Why now:** INC-11 is the next increment the plan allows without a gate (entry: INC-6 and INC-7 exits, both met). Its spec leaves the points below open, and the project rule is not to invent missing architecture.

## Decisions

| # | Question | Facts | Options | [R] |
|---|---|---|---|---|
| D1 | How is a connecting shell checked as "same uid" (C7.1 `SO_PEERCRED`)? | [F] `UnixStream::peer_cred` is still unstable on the pinned Rust 1.99 (E0658, tried). [F] `tools/verify-architecture` `ALLOWED_EXTERNAL` gives `infynitae-ipc` and the binary no external crate; `infynitae-ipc` forbids `unsafe`. [F] `rustix` is already in the lockfile (Smithay uses it). | (a) the **binary** accepts connections and checks `SO_PEERCRED` with `rustix` (safe API), adding `rustix` to the binary's allowed list; `infynitae-ipc` stays transport-agnostic (core only) · (b) `libc` + one listed `unsafe` site (U-007) in the binary · (c) amend C7.1 so that admission is the 0700 directory + 0600 socket only | **(a)** |
| D2 | Wire format | [F] C7.1 allows "a serialization crate" in `infynitae-ipc`; none chosen. Shells (Quickshell, scripts with `jq`) read JSON. | (a) JSON lines, encoder and strict parser hand-written in `infynitae-ipc`, **no new dependency** · (b) `serde` + `serde_json` · (c) a binary format | **(a)** |
| D3 | The command set of INC-11 (C7.2 is "illustrative, not frozen") | — | Snapshot, Subscribe; `ViewportFit{output, element}`, `ViewportRestore{output}` (existing fit/restore paths); `Focus{element}`, `AcknowledgeAttention{element}`; `MoveElement{element, to}`; `Navigate{output, to: Element \| CanvasPos \| Delta}` | that set, **Navigate at the current zoom only** (no zoom change; see D5) |
| D4 | `Navigate` makes OB-24's U-1 reachable | [F] One `Navigate` can move a camera by more than 2³¹ physical px. F-LATTICE-1 then resets the kept reference (specified behaviour). Its ±1 px residual is not accepted under OB-24 as worded (INC-10 change-control record U-1). | (a) implement F-LATTICE-1 as written and decide U-1 (b) (scoped closure + OB-60 for camera jumps) · (b) U-1 (a) erratum first · (c) defer `Navigate` | **(a)**, with U-1 option (b) decided together |
| D5 | Zoom through the shell | [F] OB-53's CameraZoom part belongs to "the increment that introduces discrete zoom stimuli (… e.g. INC-11's `Navigate`)". | (a) no shell zoom in INC-11 (CameraZoom stays with a later increment) · (b) `Navigate` carries a zoom, and INC-11 owns OB-53's CameraZoom part | **(a)** |
| D6 | `MoveElement` before INC-14 | [I] C7.2 requires "the same validated placement path as an interactive drag commit": `stage::place`, R3 rounding, ±2³⁶, mapped, not fullscreen. Interactive drag is INC-14 (gated by OB-34, the cross-output **drag** definition). A shell command names a Canvas point, not a pointer path across outputs. | (a) `MoveElement` in INC-11 through `stage::place` with C7.2's validation (no OB-34 dependency) · (b) defer it to INC-14 | **(a)** |
| D7 | Event model and generations (INV-SHELL-2) | [F] Core has no generation counter or change log. INV-ALLOC-1 forbids Core hot-path allocations. | (a) Core keeps a `u64` generation that every shell-visible change bumps (no allocation). `infynitae-ipc` builds a snapshot only when the generation moved and a subscriber exists, and sends the difference as events stamped with it. Several changes in one loop iteration share one generation; a missed event is re-synced by Snapshot (C7.3) · (b) a per-change event ring in Core | **(a)** |
| D8 | ShellStream epochs (INV-SCALE-3: "a bounded shell stream of navigation or `MoveElement` commands", ended by "explicit end-of-stream" or the settlement timeout) | — | (a) commands may carry `stream: begin \| step \| end`; outside a stream each command is a one-step epoch · (b) every shell command is a one-step epoch (OB-53's INC-11 part then needs no stream test) | **(a)**, since OB-53's INC-11 part tests exactly this |
| D9 | What "Stage metadata" in a Snapshot contains | [F] Core knows ids, state, anchor, extent, z-order, fullscreen, focus and attention. **Titles and app_ids are not in Core** (the adapter has them). | (a) ids, state, anchor, extent, z-order, presentation, focus, attention, cameras, outputs — no title/app_id in INC-11 · (b) title and app_id added to Core as derived protocol metadata (a new OBJ field) | **(b)**: a shell can't list windows usefully without them. They're display-only and never authority. |
| D10 | Attention events (OB-35) | [F] On `integration/inc12-on-main-2026-10-20` (INC-12), activation produces attention marks, and unmap and destroy clear them in `target_lost`. | INC-11 adds the shell events; OB-35's T1 test needs INC-12's activation path | **INC-12 integrates before INC-11** |

## Proposed order after these decisions

1. **Core:** a generation counter; the `ShellCommand` variants; `Core::shell_command` executes them through the existing single-writer paths, and `Core::shell_snapshot` returns a read-only copy.
2. **`infynitae-ipc`:** JSON-lines codec, connection state (OBJ-31), Snapshot, Subscribe, event diffing, request ids, `StaleElement` errors.
3. **The binary:** socket `$XDG_RUNTIME_DIR/infynitae/shell.sock` (0700/0600), `SO_PEERCRED` check (D1), fd-driven loop source (no idle wake-ups).
4. **T1 tests:** stale IDs, reconnect, command atomicity, the shell-stream epoch case (TST-23/24 on the production path), OB-35, and OB-53's INC-11 part with an adversarial client.
5. **Seeded controls and gates;** an independent review; then your exit review.
