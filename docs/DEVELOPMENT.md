# Working on Infynitae

## Layout

| Path | What it is |
| :--- | :--- |
| `crates/infynitae-core` | The spatial model: canvas, cameras, focus, input, scale, fullscreen, layers. A pure state machine: no Wayland, no I/O, `#![forbid(unsafe_code)]`. It takes events and returns effects. |
| `crates/infynitae-wayland` | The Smithay adapter: Wayland protocols in, Core's effects out to clients. Also the session loop, the lock, screencopy, clipboard, XWayland supervision. |
| `crates/infynitae-backend-drm` | Real screens: DRM/KMS, GBM/EGL, libinput, libseat, VRR, idle screen-off. |
| `crates/infynitae-backend-winit` | A session in a window inside another desktop, for trying things. |
| `crates/infynitae-backend-headless` | An off-screen backend. Its `tests/` are the bulk of the test suite: real Wayland clients against a real compositor. |
| `crates/infynitae-ipc` | The shell socket (`$XDG_RUNTIME_DIR/infynitae/shell.sock`, JSON lines). |
| `crates/infynitae` | The binary: command line, picks a backend. |
| `live/` | The desktop around the compositor: session scripts, environment, shortcut dispatcher, settings sync, and the C helpers (lock, wallpaper, portal, polkit, recorder). |
| `config/config` | The config template, installed as `~/.config/infynitae/config`. |
| `vendor/smithay-0.7.0` | Smithay, vendored with three authorized patches (`patches/`). |
| `tools/`, `enforcement/` | The checks (below). |
| `docs/foundation/` | The design specification. |
| `docs/knowledge/` | Research notes, rejected approaches, pitfalls. |
| `experiments/`, `docs/archive/` | History: early spikes and harnesses, past audits and evidence. Not current rules. |

## Build and run

```bash
cargo build                                   # debug build
cargo run -- --nested --width 1600 --height 900   # a session in a window
live/run.sh                                   # the same with the whole desktop (session.sh)
./install.sh / infynitae-update               # release build installed to ~/.local/bin
```

Never start a second compositor on the console of a running session.

## Tests and checks

All of these run in CI on every push, and all must pass before a change lands on `main`:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked          # ~780 tests, a few minutes
tools/verify-architecture                # several minutes
tools/check-foundation-docs              # and --self-test
tools/verify-knowledge --content-only    # and --self-test
tools/verify-test-efficacy               # hours: CI, or single controls by hand
```

| Check | What it guards |
| :--- | :--- |
| `verify-architecture` | The crate graph (Core depends on nothing), the exact Smithay pin and its files' hashes, the unsafe inventory (`docs/foundation/UNSAFE_INVENTORY.md`: every `unsafe` listed with its reasoning), source rules, compile probes that must fail. Each check has seeded defects it must detect. |
| `verify-test-efficacy` | 519 controls: each injects one bug into the product code and requires a named test to fail. A control whose test already fails on clean code, or times out, is reported as broken. |
| `check-foundation-docs` | The design spec is consistent: one home per namespace, the invariant set and its wording, the formulas. |
| `verify-knowledge` | Knowledge records have their fields, unique IDs, working links, and an `INDEX.md` entry. |

When a change moves code a control is anchored on, update the control's anchor and seed to the
new code (same defect), and confirm its test fails with the seed and passes without it.

## The design spec

`docs/foundation/` describes what the compositor must do, by ID. Code comments cite it.

| File | IDs |
| :--- | :--- |
| `FOUNDATION_INVARIANTS.md` | `INV-*`: what must always hold |
| `FOUNDATION_STATE_MACHINES.md` | `M1`…: the lifecycles (pointer, focus, fullscreen, outputs…) |
| `FOUNDATION_ARCHITECTURE.md` | crates and layers, formulas `F-*`, the layered amendments `C12.*` |
| `FOUNDATION_OBJECT_MODEL.md` | `OBJ-*`: Core's objects |
| `FOUNDATION_DECISIONS.md` | `ADR-*`: architecture decisions |
| `FOUNDATION_TEST_PLAN.md` | test tiers, obligations `OB-*` |
| `FOUNDATION_BLOCKERS.md`, `FOUNDATION_RISKS.md`, `FOUNDATION_EVIDENCE.md` | Smithay quirks (`W-SM-*`), risks, the evidence behind decisions |
| `FOUNDATION_IMPLEMENTATION_PLAN.md`, `PROTOTYPE_DECISION_LEDGER.md` | how it was built, increment by increment (`INC-*`) |
| `UNSAFE_INVENTORY.md` | every `unsafe` site |

Behaviour choices (keys, focus, defaults) are in [DECISIONS.md](DECISIONS.md).

## Conventions

- **Comments state the rule**, briefly, and cite the spec or the decision they come from:
  `// a dialog opens over its parent (owner, 2026-10-24)`, `// INV-INPUT-5`.
- **Core stays pure.** A Wayland or OS concern goes in the adapter or a backend; Core gets plain
  values and returns effects.
- **A client must never crash the compositor**: client input is checked, sizes saturate, and
  protocol errors go to the client.
- **No new `unsafe`** without an entry in the unsafe inventory, and no new crates without a reason
  in `Cargo.toml` (an already-locked crate is pinned to its exact version).
- **Commits**: `vysaely <vysaely@users.noreply.github.com>`; one change per commit, a message that
  says what changed and why.

## Working with AI agents

Much of the code was written by agents, and it still is. What keeps it sound:

- An agent works on a branch or in its own worktree, never directly on `main`.
- Its whole diff is read, and every claim it makes is checked against the code and the test run.
- It never runs, sources or tests anything that writes under the real `$HOME` (`~/.config`,
  `~/.local`, `~/.icons`): scripts are tested with `HOME` pointed at a scratch directory.
  (`live/settings.sh` takes `CONFIG_FILE` for this.)
- Security-relevant changes (the lock, authentication, input routing, permissions) get a test
  that performs the attack and fails without the fix.
