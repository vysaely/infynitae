# Infynitae: notes for coding agents

A Wayland compositor with an infinite canvas (Rust, Smithay 0.7 vendored), plus the desktop around it (`live/`).
The owner is not a programmer: explain in plain words. Layout, checks and how to run things: `docs/DEVELOPMENT.md`.
Current state and defaults: `docs/STATUS.md`. Decisions: `docs/DECISIONS.md`. The design spec is
`docs/foundation/` (large: open only the file and section you need). `docs/archive/` and `experiments/` are
history, hidden from search by `.ignore`.

## Rules that are easy to break

- **No slop** (`docs/NO_SLOP.md`, the owner's rule): nothing decorative that carries no information,
  no sample or invented data in anything shipped, no hype or filler words in UI text, docs, comments
  or commits, no speculative code. Read it before changing anything people see or read.
- **Core is a pure state machine** (`crates/infynitae-core`: no Wayland, no I/O, no unsafe). It takes events and
  returns effects; the adapter (`crates/infynitae-wayland`) applies them. Rules and invariants (INV-*, F-*) live
  in `docs/foundation/`; `tools/verify-architecture` and `tools/check-foundation-docs` enforce structure.
- **The owner's live desktop runs scripts from this checkout** (`~/infynitae/live/*.sh`; the compositor spawns
  `live/launch.sh`). A merge or a branch switch here changes the running desktop at once, while the binaries in
  `~/.local/bin` change only with `infynitae-update`. A script must keep working with the binaries installed
  before an update (e.g. a renamed helper falls back to its old name). Work on branches in a separate worktree.
- **Builds on the owner's PC pause the lab first** (owner 2026-11-12: "pause the ai training and just continue
  whenever done"): `~/aevyternae-mind/scripts/lab-pause on` before cargo build/check/test/clippy, `lab-pause off`
  after (training resumes from its last checkpoint). Never build beside a running training. Tests still only through
  the sandboxes (`safe-cargo`, `safe-run`): never against the live desktop. GitHub CI runs on every push (fmt,
  check, build, test, clippy, verify-architecture, verify-test-efficacy, foundation docs, C helpers); the test VM
  (`~/.cache/infynitae-vm/run/`, software rendering, its own CPUs) for what needs a screen. Lab files in
  `~/aevyternae-mind` are not ours.
- **Flaky test**: `t1_transform inc3_f09_bounded_resource_cycles` counts `/proc/self/fd` while other tests run;
  if it is the only failure, rerun the failed jobs (`gh run rerun <id> --failed`).
- **Tests**: most are real Wayland clients against the headless backend (`crates/infynitae-backend-headless/tests`,
  the `Lockstep` harness in `tests/common`). A fix comes with a test that failed before it. Tests never touch
  the owner's desktop: the harness points the compositor at an owned temporary folder with a stub `launch.sh`
  and an isolated client environment, so shortcut actions never run `$HOME/infynitae/live/launch.sh` or talk
  to the host session. It must stay so.
- **No emoji or status symbols** in anything the system shows or in docs: plain words (maths and arrows are fine).
- **Merging**: to `main` only when CI is green; commit messages end with the attribution line the session gives.
- Don't restart the owner's replay recorder (`infynitae-reload recorder`, or Super+Shift+BackSpace): it clears
  the last 20 minutes they may want to save.

## Where things are

| Need | Look in |
|---|---|
| A shortcut | `crates/infynitae-core/src/input/keys.rs`; actions run `live/launch.sh <action>` |
| Window placement, focus, cameras | `crates/infynitae-core/src/` (stage, focus, cameras, input/) |
| A protocol | `crates/infynitae-wayland/src/` (one file per protocol) |
| The shell socket (`infynitae msg …`) | `crates/infynitae-ipc` |
| Who may use capture, clipboard, input, lock… (`ALLOW_*`) | `crates/infynitae-wayland/src/trust.rs`; the model in `docs/SECURITY.md` |
| Settings and their comments | `config/config`; loading `live/config.sh`; checks `live/config-check` |
| C helpers (lock, wallpaper, polkit, portal, recorder, the old edge) | `live/<name>/`; built by `live/build-helpers.sh` |
| The edge (panels, desktop widgets) in Rust | `live/edge-rs` (own crate, CI job "Edge (Rust)"); design and settings `docs/EDGE_RUST.md`; VM tests `tools/vm-edge/` |
| Where windows open, move, grow | `docs/WINDOWS.md` |
| Install and update | `install.sh`, `update.sh`, `live/install-lib.sh` |
