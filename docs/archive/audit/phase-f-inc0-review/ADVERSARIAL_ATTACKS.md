# Adversarial Attack Log — Phase F INC-0 Review

All attacks below were executed against commit `0c229adf6c7837d7b659b887ae1c15cc09a8c8b3`. Temporary mutations were executed in isolated temporary copies and confirmed to leave the real working tree untouched.

---

## 1. Authority Model Attacks (§9)

| Attack Vector | Target / Injection | Result | Verdict |
|---|---|---|---|
| Struct literal construction of `InputPort` outside core | `enforcement/probes/src/lib.rs` (`p2`) | Rejected by rustc: private fields | BLOCKED (Compile-time) |
| Struct literal construction of `OutputLifecycleToken` | `enforcement/probes/src/lib.rs` (`p7`) | Rejected by rustc: private fields | BLOCKED (Compile-time) |
| Struct literal construction inside core outside `Core::new` | Seed in `crates/infynitae-core/src/lib.rs` | Rejected by `source-rules` (`authority-construction`) | BLOCKED (AST rule) |
| Second construction site inside `Core::new` | Seed in `crates/infynitae-core/src/lib.rs` | Rejected: `sites.len() != 1` | BLOCKED (AST rule) |
| Derive `Clone` on `InputPort` / `OutputLifecycleToken` | `crates/infynitae-core/src/lib.rs` | Rejected by `source-rules` (`authority-not-duplicable`) | BLOCKED (AST rule) |
| Use `Clone` on `InputPort` / `OutputLifecycleToken` | `enforcement/probes/src/lib.rs` (`p14`–`p17`) | Rejected by rustc: trait bound `Clone` not satisfied (E0277) | BLOCKED (Compile-time) |
| Store `InputPort` in `WaylandState` | `crates/infynitae-wayland/src/lib.rs` (`probe_store_port_in_state`) | Rejected by rustc: `*const ()` cannot be sent between threads safely (E0277) | BLOCKED (Compile-time) |
| Store `OutputLifecycleToken` in `WaylandState` | `crates/infynitae-wayland/src/lib.rs` (`probe_store_output_token_in_state`) | Rejected by rustc: E0277 | BLOCKED (Compile-time) |
| Store proof in `Core` (`sticky_fit`) | `crates/infynitae-wayland/src/lib.rs` (`probe_sticky_fit`) | Rejected by rustc: E0277 | BLOCKED (Compile-time) |
| Mint `Authorized` outside `entry` | `crates/infynitae-core/src/lib.rs` (`probe_protocol_refits_camera`) | Rejected by rustc: `mint` is private (E0624) | BLOCKED (Compile-time) |
| Mint `Authorized` in `entry` outside approved entries | Seed in `crates/infynitae-core/src/entry.rs` | Rejected by `source-rules` (`mint-location`) | BLOCKED (AST rule) |
| Call `output_connected` without token | `enforcement/probes/src/lib.rs` (`p6`) | Rejected by rustc: missing argument (E0061) | BLOCKED (Compile-time) |
| Call `output_reconfigured` without token | `enforcement/probes/src/lib.rs` (`p10`) | Rejected by rustc: missing argument (E0061) | BLOCKED (Compile-time) |
| Call `output_reconfigured` with `InputPort` | `enforcement/probes/src/lib.rs` (`p11`) | Rejected by rustc: mismatched types (E0308) | BLOCKED (Compile-time) |
| Pass camera arguments in `OutputDescriptor` | `enforcement/probes/src/lib.rs` (`p8`, `p12`) | Rejected by rustc: no field named `center` / `zoom_milli` (E0560) | BLOCKED (Compile-time) |
| Inject camera mutation into `output_reconfigured` | Seed in `crates/infynitae-core/src/entry.rs` | Rejected by `source-rules` (`reconfigured-no-camera-authority`) | BLOCKED (AST rule) |
| Token getter returning `&mut OutputLifecycleToken` | Injected method in `crates/infynitae-core/src/lib.rs` | **PASSED verifier undetected!** | **FINDING INC0-F01 (Level B)** |

---

## 2. AST Source-Rules Checker Attacks (§10)

### Reproduction of Finding INC0-F01: Reference Getter Blindspot
In `enforcement/source-rules/src/main.rs`:
```rust
fn by_value_names(ty: &syn::Type, out: &mut Vec<String>) {
    match ty {
        syn::Type::Reference(_) | syn::Type::Ptr(_) => {}
        ...
```
When an adversarial getter returning a reference is added to `crates/infynitae-core/src/lib.rs`:
```rust
impl Core {
    pub fn token_ref(&mut self) -> &mut OutputLifecycleToken { loop {} }
}
```
`infynitae-source-rules --root <temp>` exits with `0`, reporting `ok authority-return`.  
The AST checker fails to catch getters that return authority types by reference or pointer.

---

## 3. Dependency DAG Attacks (§14)

| Attack Vector | Target Injection | Result | Verdict |
|---|---|---|---|
| Normal dependency in `infynitae-core` | `smithay = "=0.7.0"` in `crates/infynitae-core/Cargo.toml` | Detected: declared edge is forbidden | BLOCKED |
| Workspace cycle `wayland -> backend` | `infynitae-backend-headless` in `crates/infynitae-wayland/Cargo.toml` | Detected: declared edge is forbidden | BLOCKED |
| Forbidden edge `ipc -> wayland` | `infynitae-wayland` in `crates/infynitae-ipc/Cargo.toml` | Detected: declared edge is forbidden | BLOCKED |
| Forbidden edge `backend -> backend` | `backend-headless` in `crates/infynitae-backend-winit/Cargo.toml` | Detected: declared edge is forbidden | BLOCKED |
| Product -> Enforcement dependency | `infynitae-architecture-probes` in `crates/infynitae/Cargo.toml` | Detected: product crate depends on enforcement crate | BLOCKED |
| Product -> Binary enforcement dependency | `infynitae-source-rules` in `crates/infynitae/Cargo.toml` | Detected: product crate declares external/unallowed crate | BLOCKED |
| Dev-dependency in `infynitae-core` | `[dev-dependencies] infynitae-wayland` in `crates/infynitae-core/Cargo.toml` | **PASSED verifier undetected!** | **FINDING INC0-F02 (Level B)** |

### Reproduction of Finding INC0-F02: Dev-Dependency Filter Skip
In `tools/verify-architecture` lines 300-301:
```python
for d in p["dependencies"]:
    if d["kind"] not in (None, "build"):
        continue
```
`d["kind"] == "dev"` is skipped. When `[dev-dependencies]` is injected into `crates/infynitae-core/Cargo.toml` pointing to `infynitae-wayland` or `smithay`, `tools/verify-architecture` reports:
`ok infynitae-core has no dependencies`
This violates `INV-ARCH-1` and allows forbidden test dependencies into the pure core crate.

---

## 4. Compile-Probe Judge Attacks (§11)

| Attack Scenario | Test Setup | Result |
|---|---|---|
| Probe unexpectedly compiles | Positive control `p5` judged against error `E0624` | Rejected as `VIOLATION` |
| Wrong compiler error code | `p1` (fails with `E0624`) judged against `E0603` | Rejected as `WRONG REASON` |
| Wrong rendered phrase | `p1` judged against `("code", "E0624", "wrong_phrase")` | Rejected as `WRONG REASON` |
| Harness failure (missing feature) | Nonexistent feature `no_such_probe_feature` | Rejected as `HARNESS ERROR` |
| Multiple compiler errors | Injected secondary syntax error into probe | Rejected as `WRONG REASON` (requires `len(errors) == 1`) |
| Error from wrong file | Injected error in dependency crate | Rejected as `WRONG REASON` (requires span in probe's own file) |

Verdict: The compile-probe judge logic is sound, requires exactly 1 diagnostic, requires the primary span in the probe's file, and checks both error code and semantic substring.

---

## 5. Unsafe Policy Attacks (§13)

| Attack Vector | Injection | Result | Verdict |
|---|---|---|---|
| `unsafe {}` in `infynitae-core` | Injected into `crates/infynitae-core/src/lib.rs` | Rejected by rustc: lint `unsafe_code` forbidden | BLOCKED |
| `#[allow(unsafe_code)]` in `infynitae-core` | Injected into `crates/infynitae-core/src/lib.rs` | Rejected by rustc: error `E0453` (allow incompatible with forbid) | BLOCKED |
| `unsafe {}` in `infynitae-ipc` | Injected into `crates/infynitae-ipc/src/lib.rs` | Rejected by rustc: lint `unsafe_code` forbidden | BLOCKED |
| `unsafe {}` in `infynitae-wayland` | Injected into `crates/infynitae-wayland/src/lib.rs` | Rejected by rustc: lint `unsafe_code` denied | BLOCKED |
| `#[allow(unsafe_code)]` in `infynitae-wayland` | Injected into `crates/infynitae-wayland/src/lib.rs` | Rejected by `source-rules`: `allow(unsafe_code) without UNSAFE_INVENTORY.md` | BLOCKED |
| Remove `#![forbid(unsafe_code)]` from core | Injected into `crates/infynitae-core/src/lib.rs` | Rejected by `source-rules`: `#![forbid(unsafe_code)] missing` | BLOCKED |
