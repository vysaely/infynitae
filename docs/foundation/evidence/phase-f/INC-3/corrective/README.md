# INC-3 Corrective Verification and Ratification

## History

| Step | Commit / record | Result |
|---|---|---|
| INC-3 implementation | `6470e1a` (code), `26efeb6` (evidence) | Implemented; OB-45 OPEN pending review |
| Independent review | Dossier [`audit/phase-f-inc3-review/`](../../../../audit/phase-f-inc3-review/), recorded unchanged in `85d128a` | **INC-3 CONDITIONAL PASS — CORRECTIONS REQUIRED BEFORE RATIFICATION**; INC3-F09 (Level B), INC3-F08 (Level C), Observations INC3-F01...F07 |
| Corrective patch (INC3-F09) | `0187344` `fix(test): resolve INC3-F09 file descriptor exhaustion and add regression test` | Single shared shm pool in Probe; Lockstep Drop client teardown and server dispatch; 20-cycle regression test; production code unchanged |
| Tooling isolation (INC3-F06) | `ffc9725` `fix(tooling): isolate verify-test-efficacy target directory per run (INC3-F06)` | Isolated `target/test-efficacy-<pid>-<rand>` per invocation with cleanup |
| Corrective verification | Outputs in this directory | All PASS (below) |
| Ratification | `docs(foundation): ratify INC-3, close OB-45, and record corrective evidence` | OB-45 `CLOSED`; INC-3 ratified |

The review's verdict and findings stand as written. This record does not replace them.

---

## INC3-F09 — File-Descriptor Exhaustion under Standard Limit (Level B): RESOLVED

### Problem Statement
Under an ordinary host file-descriptor limit:
```sh
ulimit -n 1024
cargo test --workspace --locked
```
The test suite failed with `TooManyOpenFiles` (os error 24) around:
`t1_transform::ee1_c5_render_hit_agreement_on_the_production_path`.

### Distinction: Leak vs Peak Resource Usage
The resource investigation established that INC3-F09 was a combination of:
- **Class A (Genuine descriptor leak):** Descriptors genuinely remained open beyond their intended lifetime. In `wayland-backend` (Rust backend), client resource destructors (including shared memory file descriptors) are executed during `cleanup()`, which `dispatch_all_clients` only calls if an epoll event is triggered for that client. When a `Lockstep` instance was dropped without an explicit kill/dispatch sequence, the server never dispatched the dead client, leaving all associated shm file descriptors permanently open for the remainder of the test process.
- **Class D (Incorrect Drop/teardown implementation):** `Lockstep` lacked a `Drop` implementation. Rust dropped fields in declaration order (`client_conn`, `display`, etc.), terminating the client socket before the server-side display state could perform ordered disconnection.
- Compounded by **Test-Harness Descriptor Multiplication:** `Probe::buffer` created an anonymous temporary file and a new `WlShmPool` for every single subsurface (192 subsurfaces = 192 files/pools in `tile_view`), keeping all file descriptors alive in `self.files` (193 client FDs + 193 server FDs = 386 FDs per iteration). Across the 10 iterations of `ee1_c5`, FDs grew monotonically at +386 FDs per iteration until hitting 1024 on iteration 4.

### Corrective Implementation
Production code remained unchanged. The test harness was corrected in `crates/infynitae-backend-headless/tests/common/mod.rs`:
1. **Single Shared Shm Pool in `Probe`:** Replaced `files: Vec<File>` and per-buffer pool creation with a single dynamically-resizing shared anonymous shm file and `WlShmPool`, reusing offsets across all buffers and subsurfaces. Subsurface buffers now consume 1 client FD and 1 server FD total per probe client, regardless of surface count.
2. **Deterministic Server Teardown in `Lockstep::drop`:** Added the `Client` handle to `Lockstep` and implemented `Drop for Lockstep`. On drop, `Lockstep` explicitly kills the client via `self.display.backend_handle().kill_client(self.client.id(), ...)` and immediately invokes `self.display.backend().dispatch_single_client(&mut self.state, self.client.id())` to purge server buffers and close all associated server descriptors.

### Resource Measurements
| Measurement Point | Before Correction | After Correction |
|---|---|---|
| Baseline before test | 9 FDs | 9 FDs |
| Peak during `ee1_c5` | > 1939 FDs (crashed at 1024) | 14 FDs |
| Peak during full workspace suite (122 tests) | N/A (failed) | 23 FDs |
| Post-test return | Leaked (> 1024) | 9 FDs (0 leaked) |

### Regression Test
Added permanent regression test `inc3_f09_bounded_resource_cycles` in `crates/infynitae-backend-headless/tests/t1_transform.rs`:
- Executes 20 repeated cycles of `Lockstep::new` + `tile_view` (192 subsurfaces) + `drop(ls)`.
- Inspects `/proc/self/fd` on Linux to assert that net file descriptor growth across 20 full cycles is $\le 5$ descriptors.

---

## INC3-F06 — Concurrent `verify-test-efficacy` Target Directory Race (Observation): RESOLVED

### Adjudication
The independent review observed that `tools/verify-test-efficacy` shared a hardcoded target directory `target/test-efficacy`. Concurrent runs (e.g. running `verify-development-environment` or running efficacy in parallel) could overwrite compilation artifacts, leading to potential false PASS (testing a stale binary), false MISS, corrupted binaries, or lock contention.

### Resolution
In `tools/verify-test-efficacy`, the static global `TARGET` was replaced with an isolated target directory per process run:
```python
pid = os.getpid()
rand_suffix = os.urandom(4).hex()
TARGET = f"target/test-efficacy-{pid}-{rand_suffix}"
```
with robust teardown in a `finally:` block.

---

## INC3-F08 — Popup Anchor-Output Rule 1 (Level C): ADJUDICATED

### Adjudication
The review noted that D-B6-BOUNDARY rule 1 (determining the popup's anchor output based on grab serial or parent surface) was not implemented in INC-3.
From canonical specifications (`FOUNDATION_ARCHITECTURE.md` §D-B6-BOUNDARY, `FOUNDATION_OBJECT_MODEL.md` OBJ-18, `FOUNDATION_TEST_PLAN.md` OB-4):
- Popup trees, `xdg_popup.grab`, grab serials, and popup dismissal belong strictly to **INC-4** (`OB-4` / `OB-18`).
- In INC-3, headless surfaces are placed at (0,0), and `xdg_popup.grab` is unhonored (no client grab can exist before INC-4).
- Rule 1 cannot be meaningfully implemented or exercised without INC-4's `xdg_popup.grab` and `ActiveElementFocus`.
- Therefore, the review's Level C disposition is formally validated: **D-B6-BOUNDARY Rule 1 is owned by INC-4 / OB-4**.

---

## Obligation Statuses

| Obligation | Scope / Description | Current Status | Assigned Owner Increment / Tier |
|---|---|---|---|
| `OB-45` | Render / hit agreement across output transforms | **CLOSED** | EE-1 C5 verified on production path across 10 transform cases under `ulimit -n 1024` with 0 mismatches; sealed control fails 9/9 non-Normal cases |
| `OB-50` | Zero-output lifecycle | OPEN | INC-3 pointer part satisfied (EE-3); constraint release in INC-5 |
| `OB-3` | Output removal without interaction state machine | OPEN | INC-3 CameraPan orphaning satisfied (C4.6); constraint reconciliation in INC-5 |
| `OB-21` | Physical event intake and pointer arithmetic | OPEN | INC-3 T0 AST & port token authority satisfied; T4 libinput intake in INC-4 |
| `OB-4` | Popup grab acquisition and dismissal | OPEN | Precondition ordering established in INC-3; popup grab handling in INC-4 |
| `OB-49` | Output reconfiguration & runtime 2i | **CLOSED** | Ratified in INC-2 |
| `OB-2` | Exact-integer unprojection | **CLOSED** | Ratified in INC-1 |
| `OB-26` | Discrete camera lattice unprojection | **CLOSED** | Ratified in INC-1 |
| `OB-19` | Canonical workspace shape & authority | **CLOSED** | Ratified in INC-0 |
| `OB-36` | CI workflow enforcement | **CLOSED** | Ratified in INC-0 |

---

## Verification Summary (Outputs in this Directory)

- **Cargo under `ulimit -n 1024`:**
  - `cargo check --workspace --locked`: PASS
  - `cargo build --workspace --locked`: PASS
  - `cargo test --workspace --locked`: PASS (122 tests: 66 core T0, 50 distinct T1, 6 oracle self-tests)
  - `cargo fmt --all -- --check`: PASS
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS
- **`tools/verify-architecture`:** PASS
  - 31 compile probes asserting exact diagnostics;
  - 54 seeded controls detected;
  - Runtime bypass switch and 1:1 binding verified.
- **`tools/verify-test-efficacy`:** PASS
  - 78/78 seeded behavioural defects detected (including INC3-F09 regression);
  - Judge controls verified;
  - Canonical tree byte-identical after run.
- **Foundation Documentation:**
  - `tools/check-foundation-docs`: PASS (57 invariants, 20 formulas, 51 obligations intact);
  - `tools/check-foundation-docs --self-test`: PASS (18/18 seeded defects detected).
- **Historical C1 Spike:**
  - `experiments/c1-authority-spike/verify.sh`: PASS.
- **Development Environment:**
  - `tools/verify-development-environment`: PASS.
