# Third Review — INC-3 Regression Gate, Resources, Verification

## INC-3 gate — **PASS (no INC-4 regression)**
All INC-3 suites pass at HEAD (input 17, adversarial 13, transform 7 incl. `inc3_f09_bounded_resource_cycles`, outputs 10, parity 6, slice 4); every INC-3 efficacy control detected; verify-architecture [5b]/[5c] (2i, INV-INPUT-3 bypass, port binding) pass. CameraPan with popup: no client events during the pan; pan end identical to the control. Zero-output grab survives preemption at zero outputs (ATK-12 restored, t06d) and is removed by the first output. INC-3 test edits only route Super through the keyboard (required by INC4R-07), assertions unchanged. Efficacy isolation code unchanged since the live concurrency check of the first review (diff-verified, not re-run concurrently — conservative given the shutdown).
**Latent ratified-increment defect found:** R2-01 (destroyed-popup input hole) reproduces on `1ee520fa`; not a regression, but blocking for INC-4's popup paths.

## Resources (`ulimit -n 1024`)
| Run | fds | history | key/button tables | elements | grabs | violations |
|---|---|---|---|---|---|---|
| t01g 160 cycles | — | 1 | 0/0 | 2 | kb no, **ptr yes (stale)** | 0 |
| T17 150 cycles | 10→10 | 1 | 0/0 | 1 | none | 0 |
Serial provenance store capped at 64 (VecDeque). No unbounded growth observed; the cap itself is unguarded by tests (M08). T17 ends with seat XKB Super depressed (R2-04).

## Verification matrix (scratch worktree of HEAD) — all PASS
cargo check, build, test (185), fmt, clippy `-D warnings`; verify-architecture; verify-test-efficacy (98+2); check-foundation-docs; `--self-test`; c1-authority-spike; verify-development-environment. CI 37433415984 (headSha `6819c8a5e28b…`): foundation-docs, verify-architecture, workspace-build success.
