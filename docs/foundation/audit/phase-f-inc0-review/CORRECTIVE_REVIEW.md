# INC-0 Corrective Patch & Ratification Review

Date: 2026-10-05  
Baseline Under Corrective Review: `0c229adf6c7837d7b659b887ae1c15cc09a8c8b3`  
Audit Type: Independent Adversarial Corrective Verification & Ratification  

---

## 1. Provenance & Workflow

The canonical history of Phase F INC-0 proceeds as follows:

```
[aa8e1d2] Pre-INC-0 Production Baseline (Phase E Pass)
    │
    ▼
[632b1d0] feat(workspace): INC-0 production workspace and architecture enforcement
[0c229ad] docs(foundation): record INC-0 evidence and checkpoint
    │
    ▼
Independent Adversarial Review (`docs/foundation/audit/phase-f-inc0-review/`)
Verdict: INC-0 CONDITIONAL PASS — SPECIFICATION RATIFICATION REQUIRED BEFORE INC-1
Findings: INC0-F01 (Level B), INC0-F02 (Level B), INC0-F03 (Level C), INC0-F04 (Observation)
    │
    ▼
Corrective Patch (This Phase)
- Fix INC0-F01 in `enforcement/source-rules/src/main.rs` + permanent seeded negative controls
- Fix INC0-F02 in `tools/verify-architecture` + permanent seeded negative controls
- Ratify Decision D (runtime 2i validation transferred to INC-2 / OB-49)
- Synchronize living `docs/foundation/FOUNDATION_TEST_PLAN.md` (OB-19, OB-36, OB-49)
- Update `tools/check-foundation-docs` and `CHECKPOINT.md` to reflect CLOSED status of OB-19 and OB-36
    │
    ▼
Corrective Verification (All local tests + CI green)
    │
    ▼
INC-0 Final Ratification
```

---

## 2. Corrective Dispositions

### Finding INC0-F01 (LEVEL B) — AST Source-Rules Reference Getter Blindspot

- **Original Finding:** `enforcement/source-rules/src/main.rs` ignored `syn::Type::Reference` and `syn::Type::Ptr`, allowing functions returning protected authority types by reference or pointer to bypass the `authority-return` rule.
- **Reproducer:** Adding `pub fn token_ref(&self) -> &OutputLifecycleToken { loop {} }` to `crates/infynitae-core/src/lib.rs` passed `tools/verify-architecture` undetected.
- **Correction Applied:**
  1. Replaced `by_value_names` with `authority_names_in_type(ty: &syn::Type, out: &mut Vec<String>, impl_ty: Option<&str>)` which recursively inspects references (`&T`, `&mut T`), pointers (`*const T`, `*mut T`), tuples, parentheses, arrays, slices, bare function pointer return types, and generic arguments (e.g. `Option<&T>`, `Result<&mut T, E>`).
  2. Added contextual `Self` resolution to map `Self` inside impl blocks to the underlying type (flagging `impl OutputLifecycleToken { pub fn leak(&self) -> &Self }`).
  3. Added fixed-point alias and re-export collector `collect_authority_types(files: &[SourceFile]) -> HashSet<String>` resolving `type Alias = ProtectedType;` and `use ProtectedType as Renamed;`.
  4. Extended `CoreVisitor` to inspect trait items (`visit_trait_item_fn`).
  5. Updated rule `R2` (`authority-return`) to assert that only `Core::new` may return ports/tokens and only `mint` may return proofs, flagging any authority return with `"(authority escape)"`.
- **Regression Controls:** Added 6 permanent seeded negative controls to `SOURCE_SEEDS` in `tools/verify-architecture`:
  - `F01: token getter returning shared reference (&OutputLifecycleToken)`
  - `F01: token getter returning mutable reference (&mut OutputLifecycleToken)`
  - `F01: port getter returning shared reference (&InputPort)`
  - `F01: port getter returning mutable reference (&mut ShellPort)`
  - `F01: proof getter returning pointer (*const Authorized)`
  - `F01: authority escape through type alias`
- **Fresh Adversarial Result:** Every seeded mutation is detected by `tools/verify-architecture` with exit status 1 and error `FAIL authority-return:`. Legitimate references (`id_ref(&self) -> &u64`, `name_slice(&self) -> &[u8]`) continue to be accepted.
- **Disposition:** RESOLVED (LEVEL B CLOSED).

---

### Finding INC0-F02 (LEVEL B) — Dependency DAG Verifier Skips `[dev-dependencies]`

- **Original Finding:** `tools/verify-architecture` filtered `d["kind"] not in (None, "build")`, ignoring `dev-dependencies`. If `crates/infynitae-core/Cargo.toml` declared `[dev-dependencies]`, `dag_problems` reported ok.
- **Reproducer:** Adding `[dev-dependencies] infynitae-wayland = { path = "../infynitae-wayland" }` to `crates/infynitae-core/Cargo.toml` passed `tools/verify-architecture` undetected.
- **Correction Applied:**
  1. Updated `direct(pid)` and `closure(pid)` in `dag_problems` to inspect `{None, "build", "dev"}` across resolved dependency information.
  2. Added explicit assertions that `infynitae-core` has zero dependencies of any class (`deps` is empty in resolve, and `pkg["dependencies"]` is empty in declared manifests).
  3. Updated declared manifest iteration to inspect all dependency classes: `(None, "build", "dev")`, checking target-specific and optional dependencies.
  4. Added explicit rejection of product crates declaring or resolving enforcement crates across all dependency classes.
- **Regression Controls:** Added 6 permanent seeded negative controls to `GRAPH_SEEDS` in `tools/verify-architecture`:
  - `F02: core gains a dev-dependency (smithay)`
  - `F02: core gains a dev-dependency (wayland)`
  - `F02: core gains a build-dependency (smithay)`
  - `F02: core gains a target-specific dependency`
  - `F02: ipc -> wayland dev-dependency edge`
  - `F02: product crate gains dev-dependency on enforcement crate`
- **Fresh Adversarial Result:** Every seeded mutation is detected by `tools/verify-architecture` with exit status 1.
- **Disposition:** RESOLVED (LEVEL B CLOSED).

---

### Finding INC0-F03 (LEVEL C) & Finding INC0-F04 (Observation) — OB-19 Spec Sync & 2i Transfer

- **Analysis:**
  - `FOUNDATION_TEST_PLAN.md` lines 273–285 previously omitted E1R amendments 2h and 2i.
  - Step 2i (`OutputKey` collision negative control `probe_sealed_key_steal`) tests runtime output-to-camera binding allocation, which cannot honestly execute in INC-0 without writing product behavior.
  - In accordance with Decision D of the independent review, runtime 2i validation is explicitly transferred to **INC-2 / OB-49** (the increment that introduces runtime output binding).
  - Invariants `INV-CAMERA-3` (1:1 camera binding) and `INV-OUTPUT-2` (Output lifecycle authority) remain fully mandatory without waiver.
- **Correction Applied:**
  1. Updated `docs/foundation/FOUNDATION_TEST_PLAN.md`:
     - Synchronized OB-19 to record E1R 2h and the transfer of runtime 2i to INC-2 / OB-49 via Decision D. Ratified OB-19 as `CLOSED`.
     - Synchronized OB-36 to record CI workflow execution evidence and ratified OB-36 as `CLOSED`.
     - Updated OB-49 to explicitly own the runtime 2i validation (`probe_sealed_key_steal` / `concurrent_output_key_collision`) transferred from OB-19, asserting that OB-49 cannot close without this runtime evidence.
  2. Updated `tools/check-foundation-docs` to enforce that OB-19 and OB-36 have status `CLOSED` while preserving `PHASE-F-ENTRY-GATE` destination and `unconditional` entry gate blocks.
  3. Verified `tools/check-foundation-docs` and `tools/check-foundation-docs --self-test` (18/18 seeded defects detected) pass 100%.
  4. Updated `docs/foundation/CHECKPOINT.md` to record the ratified state: `PHASE F — INC-0 RATIFIED — READY FOR OWNER AUTHORIZATION OF INC-1`.
- **Disposition:** RESOLVED & RATIFIED.

---

## 3. Fresh Adversarial Mutation Re-Evaluation

The 8 required mutation categories were explicitly re-evaluated:

| Mutation Category | Specific Injection | Result |
|---|---|---|
| **A. Shared Reference Getter** | `pub fn token_ref(&self) -> &OutputLifecycleToken` | BLOCKED (`FAIL authority-return: ... (authority escape)`) |
| **B. Mutable Reference Getter** | `pub fn token_mut(&mut self) -> &mut OutputLifecycleToken` | BLOCKED (`FAIL authority-return: ... (authority escape)`) |
| **C. Other Protected Types** | `pub fn input_port_ref(&self) -> &InputPort`, `pub fn shell_port_mut(&mut self) -> &mut ShellPort`, `pub fn proof_ptr(&self) -> *const Authorized` | BLOCKED (`FAIL authority-return`) |
| **D. Forbidden Normal Dep** | `smithay` declared in `crates/infynitae-core/Cargo.toml` | BLOCKED (`infynitae-core has dependencies`) |
| **E. Forbidden Dev Dep** | `[dev-dependencies]` in `infynitae-core` or `infynitae-ipc` | BLOCKED (`infynitae-core has dependencies` / `forbidden edge`) |
| **F. Forbidden Build Dep** | `[build-dependencies]` in `infynitae-core` | BLOCKED (`infynitae-core has dependencies`) |
| **G. Target-Specific Dep** | `[target.'cfg(unix)'.dependencies]` in `infynitae-core` | BLOCKED (`infynitae-core has dependencies`) |
| **H. Existing Negative Controls** | All 26 negative compiler probes + 24 original seeds + 3 judge controls | BLOCKED (All 100% detected) |

---

## 4. Final Disposition

- Finding INC0-F01: **CLOSED**
- Finding INC0-F02: **CLOSED**
- Finding INC0-F03: **CLOSED**
- Finding INC0-F04: **RATIFIED (Decision D)**
- OB-19: **CLOSED**
- OB-36: **CLOSED**

All architectural, enforcement, and specification criteria for INC-0 are satisfied.
INC-0 is fully verified and ratified.
No INC-1 compositor behavior has been implemented.
Advancement to INC-1 requires explicit project owner authorization.
