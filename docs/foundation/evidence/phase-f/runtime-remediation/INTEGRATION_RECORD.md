# Runtime remediation — integration record

Status: **integrated on local canonical `main`; EXPERIMENTAL pending INC-13; not ratified.** This
file is evidence, not authority. The runtime it hardens is the runnable prototype recorded in
`CHECKPOINT.md` ("EXPERIMENTAL RUNNABLE PROTOTYPE"); INC-13's entry conditions and exit tests
are unaffected and unmet.

## 1. Commits

Merge `59723f2` of `remediation/a-runtime` (`e53cb3b`, five commits on `db254c2`) into `main` at
`85a74cb`, followed to the Core ratification (`fa39930`). Runtime commits, oldest first:

| Commit | Subject |
|---|---|
| `eb008af` | fix(runtime): presentation timestamps on the clock wp_presentation announces |
| `95bbd1c` | test(render): pixel tests of the production render path |
| `348589c` | refactor(runtime): one session for both backends; frames on a fixed cadence, only on demand |
| `133c58c` | fix(runtime): frames for a vanished client, under a frame limit, and after a failed frame |
| `e53cb3b` | chore(verify): repair the presentation-clock control after the session change |

One merge conflict, `tools/verify-test-efficacy` (both sides appended controls): both lists kept,
146 controls, no duplicate.

## 2. Owner policies (2026-10-13)

1. The runtime remains **experimental pending INC-13**.
2. Pixman is **restricted to tests and development fixtures**: Smithay's `renderer_pixman`
   feature is a dev-dependency of `infynitae-backend-headless` only. Checked: `pixman`,
   `pixman-sys` and `paste` are absent from the normal and build dependency graph of every
   workspace crate, and no file under any `src/` names pixman.
3. The headless runner produces frames **on demand**, never as a continuous loop.
4. Shared runtime code in the adapter crate (`infynitae_wayland::session`) is accepted
   **provisionally**: it takes no authority port, and socket, client admission, child
   supervision, run limits and frame cadence each exist once.

## 3. Scope

- Presentation timestamps are read from the clock `wp_presentation` announces (review finding F2).
- `Session` (socket, display source, client dispatch, the `--exec` child with a pidfd wake, run
  limits, shutdown) and `FrameTimer` (fixed cadence from the previous deadline, armed only on
  demand) replace the two copied run loops (F3, F4). No trait; each backend keeps its loop body.
- A client that vanishes still gets its repaint; a frame limit ends an idle session; a failed
  frame is retried.
- Pixel tests of the production render path with the software renderer (F6, software part).

## 4. Tests

Added: `t1_pixels` (4 tests: a buffer on exactly its pixels; subsurface and window stacking;
scaled and clipped surfaces against Core's render list; damage following commits and nothing
else); 9 session and frame-timer unit tests; presentation-clock and vanished-client socket
tests. 20 seeded controls. Workspace total 263 tests (257 before).

Verification at the integrated commit is recorded in `CHECKPOINT.md`.

Measured 2026-10-13 on the rehearsal of the same runtime sources (`8efa7d8`), headless, virtual
clock, three runs: idle with no client 0.0 wake-ups/s (prototype loop: 59.8 at 60 Hz, 163–164.5
at 165 Hz); one client at 165 Hz 164.92–165.00 callbacks/s (prototype loop: 163.22–164.12).
These are proxies of the loop's pacing arithmetic. They are not displayed frames, not C14
baselines and not hardware measurements.

## 5. Limitations and open findings

- **B-1:** SIGTERM and SIGINT end the process without cleanup (socket, lock file and child are
  left). Blocked on the signal-handling dependency decision.
- **The GLES render path has no pixel test.** The software-renderer tests establish nothing about
  it. INC-13's exit tests (T2 smoke, renderer filters, rotated output under GLES, real `D_max`)
  are not run.
- Nested idle wake-ups, nested input mapping and any real application: not verified.
- `vblank_period` exists in both backends and as the same arithmetic in `FrameTimer::new`; the
  nested backend's copy is used only by its own unit test. Kept deliberately: removing it would
  remove that test for a one-line helper.
- Level C items of the remediation report §12 (retry without back-off; presentation `seq` 0 and
  tick-time timestamps; a `--max-frames` nested session without damage never ends; a
  microsecond hang-up window) are unchanged.
- The architecture text says the composition root runs the event loop; the code keeps it in the
  adapter crate because the gate forbids the binary crate from naming external crates. Open.

## 6. INC-6

The INC-6 candidate adds a scale-settlement deadline and tick to both prototype run loops. This
integration replaces those loops, so INC-6 now conflicts with `main` in both backends as well as
in Core. Resolving it needs a design step, not only a merge: Core's settlement deadline must
become a wake-up source of the session, without waking when no epoch is open (INV-IDLE-1).
