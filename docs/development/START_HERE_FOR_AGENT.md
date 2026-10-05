# START HERE — Infynitae Native (for coding agents)

You are continuing **Infynitae Native**, a Rust/Smithay spatial Wayland compositor. The architecture is complete and reviewed. Phase F is under way: INC-0 created the production workspace and its architecture enforcement; INC-1 implemented the first vertical seam (one element, one output, one camera at zoom 1, headless).

## Current state

- Phase E final verdict: **`PHASE E PASS — PHASE F MAY BEGIN`** (`docs/foundation/audit/phase-e-final/VERDICT.md`).
- Last architecture seal: E1R `1ae1b736515b8c52c3e6ba7596b22c81f7098390`; tracking `e9d36732ac4fa9e934ee1b90bb696e9545c598ab`; final review recorded in `cfb54f091bd2a19065a45b32662e8f0bbddd62fc`; this handoff is in the commit after it.
- INC-0 ratified after independent review (`audit/phase-f-inc0-review/`); OB-19 and OB-36 `CLOSED`; runtime 2i owned by INC-2 / OB-49.
- INC-1 ratified after independent review and corrective patch (evidence: `docs/foundation/evidence/phase-f/INC-1/`, record `corrective/README.md`; review `audit/phase-f-inc1-review/`); OB-2 and OB-26 `CLOSED`.
- INC-2 ratified after independent review and corrective patch (evidence: `docs/foundation/evidence/phase-f/INC-2/`, record `corrective/README.md`; review `audit/phase-f-inc2-review/`); OB-49 `CLOSED` (Decision E).
- INC-3 implemented on baseline `6470e1a` (pointer intake, ownership, CameraPan at zoom 1, ProjectionFrame routing, zero-output seat transition; evidence: `docs/foundation/evidence/phase-f/INC-3/`). **Pending independent review; not ratified.**
- **INC-4 has not begun.** It needs ratification of INC-3 and explicit owner authorization.
- `docs/foundation/CHECKPOINT.md` is the source of the current gate; `git log` gives the current HEAD.

## Workflow on a new machine / fresh environment

When resuming on another OS or new installation (e.g. Arch Linux / CachyOS):

1. **Clone repository**: `git clone https://github.com/vysaely/infynitae.git` (or extract `Infynitae-Portable.tar.zst`).
2. **Read `START_HERE_FOR_AGENT.md`**: review this document and `docs/development/RESTORE_ON_ARCH_LINUX.md` for environment prerequisites.
3. **Verify repository and environment**:
   ```sh
   tools/verify-development-environment
   tools/check-foundation-docs
   tools/check-foundation-docs --self-test
   experiments/c1-authority-spike/verify.sh
   ```
4. **Launch agent (Claude / AGY)**: navigate to repository root and start the assistant.
5. **Continue from the recorded project gate**: check `docs/foundation/CHECKPOINT.md`. Confirm Phase E PASS verdict is intact and the INC-0 state recorded there. Wait for explicit authorization from the project owner before starting any increment.

## Implementation gate

Phase F starts **only** with **INC-0**, which closes **OB-19**, the unconditional Phase F entry gate.

INC-0 is **enforcement-only**: workspace skeleton, `verify-architecture` (port of `experiments/c1-authority-spike/verify.sh` with every probe asserting its expected diagnostic), CI for it and for `tools/check-foundation-docs` (OB-36), and type skeletons only where the probes need them. **No** Wayland/protocol code, **no** Stage, CameraSet or other authoritative state, **no** product behaviour. Any probe passing, or failing for the wrong reason, stops Phase F.

INC-1 (the thin vertical seam) follows only after INC-0 exits.

## Read, in this order

1. `docs/foundation/CHECKPOINT.md`
2. `docs/development/PROJECT_HANDOFF.md` (navigation aid only)
3. `docs/foundation/FOUNDATION_IMPLEMENTATION_PLAN.md` — INC-0, INC-1, gates
4. `docs/foundation/FOUNDATION_TEST_PLAN.md` — OB-19, OB-36
5. `docs/foundation/FOUNDATION_INVARIANTS.md`; `FOUNDATION_ARCHITECTURE.md` §C1 (crates/DAG), §P0 (namespace map), §F (formulas)
6. `docs/foundation/FOUNDATION_DECISIONS.md` — ADR-08, ADR-12, ADR-13

## Verify before and after any change

```sh
tools/check-foundation-docs              # expect RESULT: PASS
tools/check-foundation-docs --self-test  # expect 18/18 seeded defects detected
experiments/c1-authority-spike/verify.sh # expect == result: PASS (historical spike)
tools/verify-architecture                # expect == verify-architecture: PASS (OB-19, production)
tools/verify-test-efficacy               # expect == verify-test-efficacy: PASS (seeded behavioural defects)
tools/verify-development-environment     # environment + workspace report (non-destructive)
cargo build --workspace --locked && cargo test --workspace --locked
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --locked -- -D warnings
```

## Rules

- **Authority:** cameras change only through `InputPort`, `ShellPort` or `OutputLifecycleToken` entry points, each minting a fresh `Authorized` proof (INV-CAMERA-1, INV-OUTPUT-2). Protocol code never moves, creates or binds cameras.
- **Canonical sources:** the living regions (`<!-- living:… -->`) of `docs/foundation/FOUNDATION_*.md` are normative. Each namespace has exactly one home (§P0).
- **Do not invent missing architecture.** If something needed is not specified, stop and ask; architecture changes go through change control (a new amendment layer with provenance), never silent edits.
- **Historical evidence is not current authority:** `docs/foundation/audit/**`, `docs/foundation/evidence/**` and `experiments/**` record what was tested and decided; do not reinterpret them as live rules.
- **Do not casually modify architecture documents.** Never edit sealed history or audit records.
- **Smithay** is pinned exactly at `=0.7.0`. Patch P1 lives only in `experiments/e1r-smithay-p1/`; adopting it is OB-11's decision (before INC-5).
- **Live system:** do not start the compositor on the live session, touch the user's Hyprland, access DRM/KMS, switch VTs, install packages or change the toolchain unless a later, explicitly authorized test requires it. Default execution class is HEADLESS.
- **Commits** only when the owner asks; author `vysaely <vysaely@users.noreply.github.com>`. Never force-push.
