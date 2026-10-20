# INC-11 — owner decision record (2026-10-20)

Status: **OWNER DECISION, 2026-10-20.** Recorded from the owner's message of 2026-10-20 ("go with
the recommendation"), in answer to `DECISIONS_PROPOSAL_2026-10-20.md` (this directory, copied
verbatim from `~/infynitae/INC11_DECISIONS_PROPOSAL_2026-10-20.md`). Each decision is the
proposal's recommendation [R] for that row.

| # | Decision |
|---|---|
| D1 | The binary accepts shell connections and checks `SO_PEERCRED` with `rustix`; `rustix` is allowed for the binary; `infynitae-ipc` keeps Core as its only dependency. |
| D2 | Wire format: JSON lines, hand-written in `infynitae-ipc`, no new dependency. |
| D3 | INC-11's commands: Snapshot, Subscribe; ViewportFit, ViewportRestore; Focus, AcknowledgeAttention; MoveElement; Navigate at the current zoom. |
| D4 | `Navigate` is implemented with F-LATTICE-1 as written, and OB-24's U-1 is decided as **option (b)**: OB-24 closes for pan, fit and restore with the owner's waiver of the T3/T4 visual-evidence clause, and a new obligation (OB-60) carries the bound reset under camera jumps. **Not applied here:** the closure needs the docs-checker change of U-8 and its own governance commit (INC-10 change-control record §6). |
| D5 | No shell zoom in INC-11; OB-53's CameraZoom part stays with a later increment. |
| D6 | `MoveElement` is in INC-11, through `stage::place` with C7.2's validation; no dependency on OB-34. |
| D7 | Events are the difference between derived copies, stamped with a generation; a missed event is re-synced by Snapshot. |
| D8 | Shell streams: commands carry `stream: begin | step | end`; outside a stream a command is a one-step epoch. |
| D9 | The toplevel's title and app_id are kept by Core as display metadata (never authority). |
| D10 | INC-12 integrates before INC-11: INC-11 is built on `integration/inc12-on-main-2026-10-20`. |

Normative consequences (C7.1's admission site, the shell vocabulary, the metadata field, the
ShellStream markers) are prepared as layer C12.F18 (`C12F18_PACKAGE_2026-10-20.md`, this
directory), which is **not adopted**: its adoption into the living registries is the owner's act.
