# INC-0 evidence — production workspace and architecture enforcement

Category ENFORCE, execution class HEADLESS. Implementation commit: `632b1d0` (the first Phase F commit). Raw outputs, both run at that commit:
- [`verify-architecture.txt`](verify-architecture.txt);
- [`test-matrix.txt`](test-matrix.txt): cargo check, build, test, fmt and clippy, the foundation checker and its self-test, and the historical spike.

**Status: OB-19 MECHANICALLY SATISFIED — PENDING INDEPENDENT REVIEW/RATIFICATION.**
- OB-19 and OB-36 stay `OPEN` in FOUNDATION_TEST_PLAN.md until the independent INC-0 review.
- This record is evidence only. It changes no invariant, formula, obligation or audit record.

## 1. Owner ruling applied (INC-0 scope)

OB-19 requires the spike's probes to run "against the real crates". INC-0 introduces no authoritative state. The project owner resolved the tension in this session as follows:
- **Real crates:** the probes compile against the real production type and API boundaries, not against a copy of the spike.
- **Shape only:** INC-0 creates only the authority *shape* the probes need. It implements no product behaviour.
- **Deferred runtime checks:** runtime checks whose behaviour belongs to a later increment are recorded as deferred and never reported as tested.

The spike remains historical evidence and is not used as an enforcement target.

**Specification inconsistency, recorded for the independent review:**
- C12.E1R.4 adds 2i to OB-19, and INV-OUTPUT-2 lists "T0 probes 2f/2h/2i (OB-19)".
- 2i, however, is a runtime test of the 1:1 binding rule (INV-CAMERA-3). It needs camera creation and binding in `output_connected`, which is INC-2 behaviour. INC-0 says "Authoritative state introduced: none".
- The living OB-19 entry in FOUNDATION_TEST_PLAN.md lists probes only up to 2R (2d–2g). It does not list E1R's 2h/2i.
- INC-0 treats 2h as required and enforces it. The runtime part of 2i is deferred to INC-2.
- The review should decide whether the wording needs a documentation clarification.

## 2. What INC-0 created

| Crate | Path | Content in INC-0 |
|---|---|---|
| `infynitae-core` | `crates/infynitae-core` | `InputPort`, `ShellPort`, `OutputLifecycleToken` (private fields, `!Send`, not Clone; constructed once each in `Core::new`); `authority::Authorized` (private `mint`, `pub(in crate::entry)`, never called); `navigation::CameraSet` (read accessor `center`, private `set_center` taking a proof, never populated); private `navigation::Authority`; `output::{OutputDescriptor, OutputKey, OutputId, Transform}`; `PhysicalEvent` and `ShellCommand` with no variants; entry points that check port binding (`ForeignPort`) and otherwise refuse (`Unsupported`); `#![forbid(unsafe_code)]` |
| `infynitae-wayland` | `crates/infynitae-wayland` | `WaylandState { core }` with a `Send` assertion; `smithay = "=0.7.0"` (`wayland_frontend`, `desktop`); no handler; `#![deny(unsafe_code)]` |
| `infynitae-backend-headless`, `infynitae-backend-winit` | `crates/…` | empty, `#![deny(unsafe_code)]`; edges to core and wayland only |
| `infynitae-ipc` | `crates/infynitae-ipc` | empty, `#![forbid(unsafe_code)]`; edge to core only |
| `infynitae` (bin) | `crates/infynitae` | `fn main() {}`; depends on all product crates |
| probe harness | `enforcement/probes` | protocol-side probes (`&mut Core`, no port) |
| source rules | `enforcement/source-rules` | `syn`-based structural rules |

**Deliberately not introduced:**
- no Stage, element, surface or window storage;
- no camera creation, binding, pan, jump, fit or zoom;
- no output storage, `OutputId` allocation, hotplug, reconfiguration or pointer state;
- no `PersistedCameras` or persistence lookup;
- no `Effect` vocabulary;
- no `ProtocolEvent` or protocol policy module;
- no `WaylandState::new` and no Smithay types in the state;
- no Smithay backend features;
- no IPC protocol or serialization dependency;
- no event loop.

## 3. OB-19 criterion → mechanism → command → evidence

The criteria come from FOUNDATION_TEST_PLAN OB-19, ARCHITECTURE §OB-19, C12.R1.3, C12.R2.2, C12.R2.3 and C12.E1R.4. Every command below runs inside `tools/verify-architecture`, except the cargo matrix rows, which are in `test-matrix.txt` and the CI `workspace-build` job.

| OB-19 criterion | Production mechanism | Check / probe | Observed (at `632b1d0`) |
|---|---|---|---|
| DAG: core has no dependencies | `crates/infynitae-core/Cargo.toml` `[dependencies]` empty | [2] resolved and declared edges; seed "core gains a dependency (smithay)" | ok; seed detected |
| DAG: no wayland → backend | manifests | [2]; seed "wayland -> backend edge" | ok; detected |
| DAG: no ipc ↔ wayland | manifests | [2] edges and transitive closure; seed "ipc -> wayland edge" | ok; detected |
| DAG: no backend ↔ backend | manifests | [2]; seed "backend <-> backend edge" | ok; detected |
| (C1.2, C7.1) external crates per crate; smithay never reaches core or ipc | manifests | [2] external whitelist and transitive check | ok |
| (enforcement separation) no product → enforcement edge | manifests | [2]; seeds "product -> enforcement edge", "product -> binary-only enforcement crate (declared only)" | ok; detected |
| probe: mutate via read accessor | private `CameraSet::set_center` | `p1_mutate_through_read_accessor` | E0624 `set_center` is private |
| probe: forge port | private fields of `InputPort` | `p2_forge_input_port` | "cannot construct `InputPort` … private fields" |
| probe: input without port | `physical_input(&mut InputPort, PhysicalEvent)` | `p3_physical_input_without_port` | E0061 |
| probe: navigation internals | `pub(crate) enum navigation::Authority` | `p4_call_navigation_internals` | E0603 enum `Authority` is private |
| positive control: read access compiles | `CameraSet::center` | `p5_control_compiles` | compiles |
| core-internal policy cannot mutate a camera | `set_center` private to `navigation` | `probe_protocol_moves_camera` | E0624 |
| `!Send` port + `Send` protocol state (E0277) | `NotSend` marker; `assert_send::<WaylandState>()` | `probe_store_port_in_state` | E0277 cannot be sent |
| 1R 2d: no protocol re-fit (proof not mintable) | `Authorized::mint` is `pub(in crate::entry)` | `probe_protocol_refits_camera` | E0624 `mint` is private |
| 1R 2e: no retained proof | `Authorized` is `!Send` | `probe_sticky_fit` (core field via wayland feature) | E0277 |
| 2R 2f: token required for connect | `output_connected(&mut OutputLifecycleToken, …)` | `p6_output_connected_without_token` | E0061 |
| 2R 2f: token unforgeable | private fields | `p7_forge_output_token` | "cannot construct `OutputLifecycleToken` … private fields" |
| 2R 2f: descriptor has no coordinates | `OutputDescriptor` fields are hardware facts only | `p8_descriptor_with_coordinates` | E0560 no field `center` |
| 2R 2f: no public restore API | no such method | `p9_public_restore_api` | E0599 |
| 2R 2f: protocol policy cannot create cameras | `mint` private | `probe_protocol_creates_camera` | E0624 `mint` is private |
| 2R 2f: token not storable in protocol state | `!Send` token | `probe_store_output_token_in_state` | E0277 |
| 2R 2g: single construction site in `Core::new`; no other function returns the token | source rule `authority-construction`, `authority-return` (syntax tree, macros included; applied to both ports and the proof too) | [4]; seeds: `reissue_token()`, public constructor, construction inside `vec![…]`, token getter | ok; all detected |
| (C12.R2.3) token/ports not Clone | no derive/impl; trait bound | `p14`–`p17` (E0277 `…: Clone` not satisfied), rule `authority-not-duplicable`; seed `derive(Clone)` | ok; detected |
| E1R 2h: reconfigure needs the token | `output_reconfigured(&mut OutputLifecycleToken, OutputId, OutputDescriptor)` | `p10` (E0061), `p11` (E0308, InputPort is not the token), `p12` (E0560 no `zoom_milli`), `p13` (E0061 extra camera argument) | all as asserted |
| E1R 2h: `output_reconfigured` carries no camera authority | body contains no `mint`, `Authorized`, `navigation`, `set_center` | rule `reconfigured-no-camera-authority`; seeds: injected `mint`, injected `navigation::` call | ok; both detected |
| E1R 2i: OutputKey collision negative control | — | **DEFERRED to INC-2** (runtime binding does not exist) | reported as deferred, not tested |
| source rule: wayland never names `InputPort`, `ShellPort`, `OutputLifecycleToken`, `physical_input`, `shell_command`, `output_connected`, `output_disconnected` (+ `output_reconfigured`, derived from INV-OUTPUT-2) | rule `wayland-names-no-authority` (every identifier, macro bodies included; probe-gated items excluded) | [4]; seeds: `use …::InputPort`, `stringify!(output_connected)` | ok; detected |
| `Authorized::mint` only in the entry module | rule `mint-location`: one definition, `pub(in crate::entry)`, in `entry::authority`; uses only in `physical_input`, `shell_command`, `output_connected`, `output_disconnected` (INV-CAMERA-1) | [4]; seeds: `mint` outside the entry points, `mint` made `pub(crate)` | ok (no uses in INC-0); detected |
| `#![forbid(unsafe_code)]` in core and ipc; `#![deny(unsafe_code)]` elsewhere | crate-root attributes; no `unsafe` and no `allow(unsafe_code)` without `UNSAFE_INVENTORY.md` (C9) | rule `unsafe-policy`; probes: unsafe block in core, ipc, wayland (lint `unsafe_code`), `allow` override in core (E0453); seeds: forbid removed, forbid weakened, unsafe in a backend, allow in wayland | ok; detected |
| `cargo build --locked` | tracked `Cargo.lock` | [1]; CI `workspace-build` | ok |
| `smithay = "=0.7.0"` | `crates/infynitae-wayland/Cargo.toml` | [1] declared requirement, resolved version and source; no `[patch]`/`[replace]`; seeds: loosened requirement, `[patch.crates-io]` | ok; detected |
| pinned `rust-toolchain.toml` | `channel = "1.99.0"` | [1] file and active `rustc`; seed `channel = "stable"` | ok; detected |
| every probe fails with its asserted diagnostic and no other error | JSON diagnostics: exit status, exactly one error, primary span in the probe's own file, code and text | [5]; judge controls: compiling probe → VIOLATION, wrong code → WRONG REASON, missing feature → HARNESS ERROR | 26/26 negative probes, 1/1 positive control; judge controls ok |
| CI-blocking | `.github/workflows/ci.yml` job `verify-architecture` | GitHub Actions | see the CI run of the INC-0 commits |

Seeded controls run on temporary copies of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates/` and `enforcement/`. A SHA-256 digest of those paths, taken before and after, is asserted identical.

## 4. OB-36

CI job `foundation-docs` runs `tools/check-foundation-docs` and `tools/check-foundation-docs --self-test` on every push and pull request. It is a separate job from `verify-architecture` (OB-19) and `workspace-build`.

## 5. Mechanically enforced in INC-0 vs deferred runtime validation

| Property | INC-0 (mechanical) | Deferred (runtime, owner) |
|---|---|---|
| Camera mutation only in `navigation`, with a proof | privacy + probes | navigation behaviour: INC-3 |
| Camera creation/binding only via the token | signature, `mint` privacy, probes p6/p7, `probe_protocol_creates_camera` | binding behaviour: INC-2 |
| 1:1 binding, collision rule (INV-CAMERA-3, 2i) | no binding API exists; no shared-camera type | **2i negative control: INC-2** (T1 OB-49) |
| Ports bound to their Core | `output_connected` foreign token → `ForeignPort` (unit test `tokens_are_bound_to_their_core`) | `physical_input`/`shell_command`: INC-3 / INC-11 (no event variants yet) |
| Reconfiguration keeps identity and camera | signature + 2h source rule | behaviour: INC-2 (OB-49) |

## 6. Not touched

- The live session, DRM/KMS, VTs and system packages were not touched.
- Smithay P1 was not applied, vendored or patched (OB-11 open).
- INC-1 has not begun.
