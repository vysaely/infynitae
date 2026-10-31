# Infynitae: notes for coding agents

A Wayland compositor with an infinite canvas (Rust, Smithay 0.7 vendored), plus the desktop around it (`live/`).
The owner is not a programmer: explain in plain words. Layout, checks and how to run things: `docs/DEVELOPMENT.md`.
Current state and defaults: `docs/STATUS.md`. Decisions: `docs/DECISIONS.md`. The design spec is
`docs/foundation/` (large: open only the file and section you need). `docs/archive/` and `experiments/` are
history, hidden from search by `.ignore`.

## Rules that are easy to break

- **Core is a pure state machine** (`crates/infynitae-core`: no Wayland, no I/O, no unsafe). It takes events and
  returns effects; the adapter (`crates/infynitae-wayland`) applies them. Rules and invariants (INV-*, F-*) live
  in `docs/foundation/`; `tools/verify-architecture` and `tools/check-foundation-docs` enforce structure.
- **The owner's live desktop runs scripts from this checkout** (`~/infynitae/live/*.sh`; the compositor spawns
  `live/launch.sh`). A merge or a branch switch here changes the running desktop at once, while the binaries in
  `~/.local/bin` change only with `infynitae-update`. A script must keep working with the binaries installed
  before an update (e.g. a renamed helper falls back to its old name). Work on branches in a separate worktree.
- **No builds on the owner's PC while it trains a model** (`systemctl --user is-active mind-labq`): no cargo
  build/check/test/clippy, no GPU use. Verify through GitHub CI (runs on every push: fmt, check, build, test,
  clippy, verify-architecture, verify-test-efficacy, foundation docs, C helpers) or in the test VM
  (`~/.cache/infynitae-vm/run/`, software rendering, its own CPUs). Lab files in `~/aevyternae-mind` are not ours.
- **Flaky test**: `t1_transform inc3_f09_bounded_resource_cycles` counts `/proc/self/fd` while other tests run;
  if it is the only failure, rerun the failed jobs (`gh run rerun <id> --failed`).
- **Tests**: most are real Wayland clients against the headless backend (`crates/infynitae-backend-headless/tests`,
  the `Lockstep` harness in `tests/common`). A fix comes with a test that failed before it.
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
| Settings and their comments | `config/config`; loading `live/config.sh`; checks `live/config-check` |
| C helpers (edge, lock, wallpaper, polkit, portal, recorder) | `live/<name>/`; built by `live/build-helpers.sh` |
| Install and update | `install.sh`, `update.sh`, `live/install-lib.sh` |
