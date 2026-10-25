# START HERE — Infynitae Native (for coding agents)

You are continuing **Infynitae Native**, a Rust/Smithay spatial Wayland compositor. The architecture is complete and reviewed. Phase F is under way: INC-0 created the production workspace and its architecture enforcement; INC-1 implemented the first vertical seam (one element, one output, one camera at zoom 1, headless).

## Current state

> **Updated 2026-10-16.** The INC-0…INC-3 history that this section used to describe is kept in
> `git log` and `docs/archive/CHECKPOINT.md`; it is no longer the current state.

- `docs/archive/CHECKPOINT.md` is the source of the current gate; `git log` gives HEAD.
- **Ratified:** INC-0 … INC-5, INC-7 (without INC-6), the Core visibility remediation.
  Published `main` = `origin/main`.
- **INC-6 (scale policy): experimental, exit NOT authorized.** Newest line:
  `candidate/inc-6-exit-2026-10-16`. Its exit waits on owner adoption of amendments
  C12.F11, C12.F12, C12.F13 (commits marked "AMENDMENT NOT ADOPTED, NEVER MERGE BEFORE
  ADOPTION"). Every other `*inc-6*` branch is an ancestor or a superseded rehearsal of it.
- **INC-8 (off-screen frame policy): experimental, not ratified** on
  `experimental/inc-8-offscreen-frames` (from `main`; independent of INC-6). Record:
  `docs/archive/evidence/phase-f/INC-8/README.md`.
- **Runtime:** experimental pending INC-13; headless and nested (winit) prototypes only. Not a
  usable desktop yet.
- **Open obligations that block exits:** OB-13, OB-42, OB-44 (INC-6); OB-16 real-time harness
  part (INC-8); OB-22 T3/T5, OB-52.

## Worktrees and branches

- Canonical repository: `~/infynitae-native` (the primary checkout stays on `main`; do not work
  in it). `~/infynitae` is an empty directory, not the project.
- Work in a dedicated worktree with its own `CARGO_TARGET_DIR` (never share a target dir
  between worktrees). Existing worktrees live under `~/.cache/infynitae-integration/`.
- One writer per worktree. Before committing, check whether another agent session is active
  on the same branch.
- Never merge to `main`, push, delete branches or adopt an amendment without the owner.

## Workflow on a new machine / fresh environment

When resuming on another OS or new installation (e.g. Arch Linux / CachyOS):

1. **Clone repository**: `git clone https://github.com/vysaely/infynitae.git` (or extract `Infynitae-Portable.tar.zst`).
2. **Read `START_HERE_FOR_AGENT.md`**: review this document and `docs/archive/development/RESTORE_ON_ARCH_LINUX.md` for environment prerequisites.
3. **Verify repository and environment**:
   ```sh
   tools/verify-development-environment
   tools/check-foundation-docs
   tools/check-foundation-docs --self-test
   experiments/c1-authority-spike/verify.sh
   ```
4. **Launch agent (Claude / AGY)**: navigate to repository root and start the assistant.
5. **Continue from the recorded project gate**: check `docs/archive/CHECKPOINT.md` and the current state above.

## Implementation gate

Each increment (FOUNDATION_IMPLEMENTATION_PLAN.md) is implemented on its own branch, gated
(`tools/verify-development-environment`), recorded under `docs/archive/evidence/phase-f/`,
independently reviewed and ratified by the owner before it reaches `main`. Architecture changes
go through a new amendment layer; never edit sealed text.

## Read, in this order

1. `docs/archive/CHECKPOINT.md`
2. `docs/archive/development/PROJECT_HANDOFF.md` (navigation aid only)
3. `docs/foundation/FOUNDATION_IMPLEMENTATION_PLAN.md` — INC-0, INC-1, gates
4. `docs/foundation/FOUNDATION_TEST_PLAN.md` — OB-19, OB-36
5. `docs/foundation/FOUNDATION_INVARIANTS.md`; `FOUNDATION_ARCHITECTURE.md` §C1 (crates/DAG), §P0 (namespace map), §F (formulas)
6. `docs/foundation/FOUNDATION_DECISIONS.md` — ADR-08, ADR-12, ADR-13
7. Before designing or changing architecture: `docs/knowledge/INDEX.md` §0 (records by upcoming work) and only the records it names. Non-authoritative engineering memory (research, decisions, measurements, rejected approaches); `docs/foundation/` wins on any conflict. Check `docs/knowledge/pitfalls/` before re-proposing an approach. Validate edits with `tools/verify-knowledge`.

## Verify before and after any change

```sh
tools/check-foundation-docs              # expect RESULT: PASS
tools/check-foundation-docs --self-test  # expect every seeded fixture detected
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
- **Historical evidence is not current authority:** `docs/archive/audit/**`, `docs/archive/evidence/**` and `experiments/**` record what was tested and decided; do not reinterpret them as live rules.
- **Do not casually modify architecture documents.** Never edit sealed history or audit records.
- **Smithay** is pinned exactly at `=0.7.0`, vendored in `vendor/smithay-0.7.0` with patch P1 (adopted for INC-5 under D-B12-PIN, OB-11 Option B).
- **Live system:** do not start the compositor on the live session, touch the user's desktop session (KDE Plasma / KWin on 2026-10-16), run test clients under `dbus-run-session`, access DRM/KMS, switch VTs, install packages or change the toolchain unless a later, explicitly authorized test requires it. Default execution class is HEADLESS.
- **Commits** only when the owner asks; author `vysaely <vysaely@users.noreply.github.com>`. Never force-push.
