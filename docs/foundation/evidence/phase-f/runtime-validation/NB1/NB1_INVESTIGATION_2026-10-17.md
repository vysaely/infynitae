# NB-1 — nested-session stall: investigation, root cause, fix (2026-10-17)

Status: **ROOT CAUSE ESTABLISHED; FIX PROPOSED** on `fix/nested-host-frame-gating-2026-10-17`
(from `main` `ca2e468`), not merged. Brief "INC-6 Integration and NB-1 Hang Investigation",
Gate 2. No GPU, latency or frame-pacing claim is made: software rendering (`/dev/dri` masked).

## 1. Builds compared

| Name | Commit | Binary | What |
|---|---|---|---|
| r1 ("earlier build") | `86b3097` (runtime line r1) | `5faa2b92…` | INC-6/8/12 rehearsal + clipboard + shutdown + satellite runtime dir |
| r2 | `c55bb70` (runtime line r2) | `5a8d90fd…` | r1 + drag refusal, INC-12 r2, session process ownership |

Diff r1 → r2 in runtime code (`git diff 86b3097 c55bb70 -- crates`): process-wide SIGCHLD handler
(signal-hook, `SA_RESTART`) and orphan reaping, child subreaper, subtree teardown, drag refusal
(on a client's `start_drag` only), activation-token pruning (on token creation only). **Nothing
in the winit backend or the renderer.** A process-wide signal handler can interrupt non-restarted
waits (`poll`, `ppoll`, timed futex) in any thread, so it was a candidate; the evidence below
rules it out for this stall.

## 2. Harness (`harness/`)

`trial.sh`: one bounded trial — the nested compositor runs in `unshare -Urnmpf --mount-proc`
(private /tmp and runtime dir, masked `/dev/dri`, private network and pid namespaces, D-Bus
disabled; KDE's X display unreachable; only KDE's Wayland socket named, for the window). An
independent watchdog (outside the sandbox) samples the compositor's main-thread state every 2 s;
a trial is a STALL when the session driver makes no progress for 30 s or the run exceeds 120 s.
On a stall, before terminating: per-thread state/wchan/syscall, `gdb thread apply all bt`,
`ss -xpe` inside the sandbox's network namespace, fds, processes; then SIGKILL to the
namespace's init (the kernel ends every process of the sandbox) and a check that no compositor
is left. The compositor runs with `WAYLAND_DEBUG=client` (its own connection to KDE only; the
session's clients have it unset). `repro.sh`: the deterministic reproducer (§4).
Harness self-tests: a normal headless run (30/30) and a forced hang (detected at 30 s, evidence
collected, sandbox killed, nothing left).

## 3. Field trials (batch b1, alternating, 15 rounds, nested; `batch-b1.txt`)

| Build | Trials | Stalls |
|---|---|---|
| r2 | 15 | 2 (rounds 3, 10) |
| r1 | 15 | 2 (rounds 7, 9) |

The earlier report's "0 of 7 for r1" was a small sample: **r1 stalls as well.** All four stalls
(`field-stalls/`) have one signature: main thread in
`render_and_present → WinitGraphicsBackend::submit → eglSwapBuffersWithDamageKHR → libEGL_mesa →
wl_display_dispatch_queue → ppoll`; winit's event thread idle in `poll`; llvmpipe threads idle;
the socket from KDE has 0 unread bytes; in the protocol trace the compositor committed a frame
and requested frame callbacks, and **KDE sent no further frame callback** (its last event
precedes the stall; e.g. b1-r2-3: last `wl_callback.done` 22:39:14.186 UTC, then ~50 s of
silence). The compositor is waiting, not deadlocked or spinning. The stalled runs coincide with
user activity at the desktop (pointer and keyboard focus moving over the test windows).

## 4. Hypothesis and deterministic reproducer

**H1:** when the host stops presenting the nested window (minimised, hidden), it stops sending
frame callbacks; the winit backend submits on its own frame timer, each submit swaps with the EGL
default swap interval (1) and waits for the previous frame's callback, so the loop blocks —
client service, the run limit and SIGTERM (whose handler only writes to the self-pipe) — until
the host presents the window again.

**Reproducer** (`repro.sh`; experiment branches only, never merged): a test hook
(`INFYNITAE_NB1_MINIMIZE_MS`) asks KDE to minimise the compositor's own window after 3 s; the
session runs an animating client (mpv, 60 fps, shm) so frames are drawn; SIGTERM at 8 s;
`--duration 15`; watchdog 30 s or 60 s. Results (`reproducer/SUMMARY.txt`):

| Binary | Window | Result |
|---|---|---|
| r1 + hook (`dacc0cf`) | minimised | STALL 2/2 (≥ 30 s), main thread in the same swap |
| r2 + hook (`bfbe98f`) | minimised, SIGTERM at 8 s | STALL 2/2 (≥ 30 s) and 5/5 (≥ 60 s) |
| r2 + hook | minimised, no SIGTERM | 1/3 exit 0 at 20.93 s (5.9 s past the limit), 2/3 STALL ≥ 60 s |
| r1, r2 + hook | not minimised (control) | 2/2 exit 0, 0.51 s after SIGTERM |

H1 holds for both builds; r2's changes are not involved.

## 5. Fix (`e5e4823`) and results

`HostFrameGate`: a frame is submitted initially and after winit's `Redraw` (which winit delivers
on the host's frame callback once a redraw is requested); a due frame otherwise waits and the
loop sleeps on its event sources (no spin). Swap interval, renderer, damage and the pacing of a
visible window are unchanged.

| Binary | Window | Result |
|---|---|---|
| r2 + fix + hook (`experiment/nb1-r2-fix-reproducer-2026-10-17`) | minimised, SIGTERM at 8 s | **5/5 exit 0 at 8.52 s** |
| same | minimised, no SIGTERM | **3/3 exit 0 at 15.55–15.56 s** (limit 15 s) |
| r2 (unfixed) vs r2 + fix, visible, 60 fps client | visible | 474 host commits in ~8 s in all four runs (frame rate unchanged) |
| r2 + fix, full scripted nested session | visible | 6/6 complete, SIGTERM → exit ~18 ms |
| main + hook, unfixed vs fixed, no SIGTERM | minimised | unfixed exit at 21.61 s, fixed at 15.58 s (main has no SIGTERM handling yet: its exit 143 runs say nothing about the loop) |

Regression coverage: T0 `a_frame_is_submitted_only_when_the_host_is_ready` (the gate); the
integration (a submit blocking on a host that withholds frame callbacks) needs a real host and is
covered by `repro.sh`, which fails on every unfixed build and passes on the fixed one. Seeded
control "NB-1: a submit does not wait for the host's frame callback" (the gate never closes):
DETECTED by the T0 test. Removing the gate's call from the render path is not caught by any
`cargo test` (no host exists there); only `repro.sh` catches it.

## 6. Implications and remaining uncertainty

- **Correctness:** while the host withholds frames, the nested compositor sends its clients no
  frame callbacks (like any hidden window's client under KDE). Its loop, run limit, signals and
  client requests keep working.
- **Portability:** winit on Wayland gates `Redraw` by frame callbacks; on an X11 host `Redraw`
  follows `request_redraw` directly, so the gate never waits longer than before.
- **Performance:** visible-window frame count unchanged (474/474); while hidden no frames are
  drawn (less work than before).
- **Not established:** exactly what KDE did at each field stall (minimise, cover, desktop switch);
  the trace shows only that frame callbacks stopped. Sample sizes are small (15+15 field trials);
  no statistical claim beyond "both builds stall, at a similar rate, with one signature".
- A test-probe race (the GTK4 paste probe exits after the primary-selection answer before the
  clipboard answer is printed) cost one result in 2 of ~37 runs, with and without the fix; it is
  the probe, not the compositor.
