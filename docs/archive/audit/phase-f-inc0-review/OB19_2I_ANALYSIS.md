# Analysis of OB-19 / 2i Runtime Validation and Transfer

## Context & Origin of 2i
In Phase E corrective checkpoint E1R (finding E-F3-03), an architectural flaw in the sealed 2R output-to-camera binding rule was identified:
When two outputs connected with identical `OutputKey`s, the sealed 2R rule searched for the first camera with that key regardless of whether it was already bound to another active output. This stole the bound camera from the existing output, violating the 1:1 binding rule of `INV-CAMERA-3`.

In the authority spike (`experiments/c1-authority-spike/verify.sh` step 2i), this defect was tested by `concurrent_output_key_collision` with feature `probe_sealed_key_steal`:
```bash
if cargo test --offline -q -p spike-wayland --features probe_sealed_key_steal concurrent_output_key_collision 2>&1 | grep -q "1:1 violated"; then
    echo "OK: sealed rule steals a bound camera"
```
In §C12.E1R.4, this test was appended to the list of probes under OB-19:
> *"OB-19 (still the unconditional Phase F entry gate) gains the probes in verify.sh step 2h ... and step 2i (`probe_sealed_key_steal` negative control)."*

## Addressing the Review Questions (A through H)

### A. Does the living architecture actually require runtime 2i validation before OB-19 can close?
The wording in `FOUNDATION_ARCHITECTURE.md` §C12.E1R.4 and the `INV-OUTPUT-2` test column states that OB-19 gains step 2i. However, `FOUNDATION_IMPLEMENTATION_PLAN.md` explicitly defines INC-0 as introducing **zero authoritative state** ("Authoritative state introduced: none").
Step 2i is a **runtime execution test** that requires:
1. An instantiated, populated `CameraSet` maintaining multiple camera slots.
2. A populated output registry tracking `OutputId` allocations.
3. Real implementation of the binding resolution logic inside `output_connected` (`bind_on_connect`).
4. Reconnection resolution logic inside `output_connected` and `output_disconnected`.

Therefore, the living architecture contains a direct semantic tension between the definition of INC-0 (strictly enforcement infrastructure without product behavior) and the inclusion of runtime test 2i under OB-19.

### B. Does C12.E1R.4 / INV-OUTPUT-2 make 2i part of the unconditional Phase-F entry gate?
Textually, yes: §C12.E1R.4 and the table row for `INV-OUTPUT-2` list 2i under OB-19.

### C. Does FOUNDATION_TEST_PLAN.md fail to incorporate a later E1R amendment?
Yes. In `FOUNDATION_TEST_PLAN.md` line 275 (the canonical living definition of OB-19), the text was never updated during E1R; it still reads:
> *"incl. 1R probes 2d/2e...; 2R probes 2f/2g: output-lifecycle token required, unforgeable, no coordinate fields, no public restore API, not storable in protocol state, single construction site"*
It omits both 2h and 2i. This is an editorial defect in `FOUNDATION_TEST_PLAN.md`.

### D. Is this merely stale wording/bookkeeping?
It is partially stale bookkeeping in `FOUNDATION_TEST_PLAN.md`, but it represents a real cross-increment boundary issue: grouping a runtime multi-output binding test (belonging to INC-2) into a compile-time enforcement bootstrap increment (INC-0).

### E. Is it a genuine contradiction?
Yes. The contradiction exists between:
- **INC-0 Scope:** ENFORCE category, HEADLESS, zero authoritative state, no compositor behavior, no camera allocation or output binding.
- **INC-2 Scope:** PRODUCT category, owns output lifecycle (`output_connected`, `output_reconfigured`, `output_disconnected`), multi-output camera binding (1:1, collision rule), and the zero-output state. It explicitly claims `OB-49` and the "OB-19 runtime counterpart (token-only output entry)".
- **2i Requirements:** Requires real output-to-camera binding behavior.

Implementing 2i in INC-0 would require writing product behavior, directly violating the prohibition against implementing compositor logic before INC-1 and INC-2.

### F. Can OB-19 legitimately be ratified after INC-0 while transferring the runtime 1:1 binding validation to a later obligation?
**YES (Decision D).**
Because:
1. INC-0 implements 100% of the compile-time authority boundaries, type privacy, token unforgeability, single construction enforcement, `#![forbid/deny(unsafe_code)]`, dependency DAG restrictions, Smithay pin, toolchain pin, and CI automation.
2. INC-0 implements all compile-time probes for 2h (`p10`–`p13`) and the source rule for `output_reconfigured`.
3. In INC-0, no cameras can be bound, created, stolen, or shared. `CameraSet` is empty and `output_connected` rejects with `Err(EntryError::Unsupported)`.
4. Runtime 1:1 camera binding is authoritatively owned by **INC-2** and validated under **OB-49** (and the runtime counterpart of OB-19).
5. Ratifying OB-19 with an explicit transfer of 2i's runtime component acknowledges reality without lowering the bar: it ensures that 2i is tested when the code that actually implements the behavior is created.

### G. Would doing so weaken a sealed invariant?
No. `INV-CAMERA-3` (1:1 output-to-camera binding) and `INV-OUTPUT-2` (Output lifecycle authority) remain completely intact. No code exists in INC-0 that violates them, and the runtime negative control `probe_sealed_key_steal` will be executed against the real production crates in INC-2 before INC-2 can exit.

### H. Would closing OB-19 now falsely claim runtime evidence that does not exist?
Not under Decision D. Under Decision D, the ratification explicitly declares:
- What is satisfied: all compile-time, type-level, source-level, and CI criteria of OB-19.
- What is transferred: the runtime multi-output binding verification (2i) to INC-2 / OB-49.
- No claim is made that runtime camera binding was tested in INC-0.

## Ratification Transfer Specification
- **What transfers:** The runtime execution of the `OutputKey` collision negative control (`probe_sealed_key_steal` / `concurrent_output_key_collision` testing that 1:1 binding is preserved).
- **Where it transfers:** To **INC-2**, owned by obligation **OB-49** (and the runtime counterpart of OB-19).
- **What remains invariant:** `INV-CAMERA-3` (strict 1:1 output-to-camera binding, no shared camera authority) and `INV-OUTPUT-2` (lifecycle authority entering core strictly through `OutputLifecycleToken`).
- **Required documentation adjustment:** Record in `FOUNDATION_TEST_PLAN.md` under OB-19 and OB-49 that the compile-time enforcement of OB-19 closed in INC-0, while the runtime 2i negative control is owned and verified in INC-2 under OB-49.
