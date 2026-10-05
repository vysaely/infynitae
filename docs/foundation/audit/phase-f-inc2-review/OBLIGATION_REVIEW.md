# Architectural Obligation Review & Adjudication

## 1. Primary Obligations Adjudication

### OB-49 — Output Reconfiguration & Runtime 2i
- **Text in Living Test Plan:**
  > Output reconfiguration on the production path (E-F2-03): `output_reconfigured` for mode, transform, scale and refresh changes; same `OutputId`; camera identity and value unchanged; tracker/view/membership rebuilt; F-RECONF-1; `wl_output`/`xdg_output` events; fullscreen configure; epoch settlement; rejection of unknown ids and key changes; reconnect tested separately. Owns runtime 2i validation (transferred from OB-19 via Decision D)...
  > Blocks: INC-2 exit.
- **Evaluation:**
  - All INC-2 reconfiguration criteria (mode, transform, scale, refresh, OutputId/camera preservation, F-RECONF-1, wire events, transactional rejections, separate reconnect) are **fully implemented and verified**.
  - Runtime 2i negative control (`probe_sealed_key_steal` / `concurrent_output_key_collision`) is **fully implemented and verified** on T0 and T1.
  - However, the "Item:" text mentions "fullscreen configure" (INC-7) and "epoch settlement" (INC-6).
- **Adjudication:**
  - Finding `INC2-F01` (Level C): This is an editorial scoping defect in OB-49's description.
  - Recommended Action: Formally ratify Decision E transferring fullscreen configure to INC-7 and epoch settlement to INC-6.
  - Status: **OPEN** (Mechanically satisfied for INC-2; pending formal specification ratification).

### OB-45 — Render / Hit Agreement across Output Transforms
- **INC-2 Scope:** Tracker sizing to `phys_O` (`EE-1 C1–C4`), surface membership in `view_O`, and damage tracking on transformed outputs.
- **Evaluation:** Tracker and membership parts verified across the 10-case EE-1 matrix. Hit-test agreement across transforms (`EE-1 C5`) canonically belongs to INC-3 (F-HIT-1 routed input).
- **Status:** **OPEN** (INC-2 portion satisfied; hit-test portion owned by INC-3).

### OB-50 — Zero-Output Lifecycle
- **INC-2 Scope:** Core zero-output state (`PointerState::NoOutput`), no coordinate or output fabrication, startup placement on first connect.
- **Evaluation:** Core state and F-RELOCATE-1 domain protections verified. Adapter zero-output pointer grab belongs to INC-3; constraint release belongs to INC-5.
- **Status:** **OPEN** (INC-2 portion satisfied; pointer grab in INC-3; constraint teardown in INC-5).

### OB-3 — Output Removal Without Interaction State Machine
- **INC-2 Scope:** Clean output removal, leave before global destruction, camera unbound and retained, pointer relocation to nearest survivor.
- **Evaluation:** Lifecycle removal and pointer relocation verified. Interaction state machine orphaning belongs to INC-3 / INC-14.
- **Status:** **OPEN** (INC-2 portion satisfied; interaction orphaning in INC-3).

---

## 2. Tracking of Remaining Open Obligations

| Obligation | Scope / Description | Current Status | Assigned Owner Increment / Tier |
|---|---|---|---|
| `OB-1` | Lattice reference vs production rasterization | OPEN | INC-6 / T1 |
| `OB-6` | Fractional scale edge rounding parity | OPEN | INC-6 / T4 |
| `OB-14` | Subsurface unmap / sync commits | OPEN | INC-9 / T1 |
| `OB-17` | Direct-scanout candidate evaluation | OPEN | INC-16 / T4 |
| `OB-48` | Multi-session persisted camera storage | OPEN | INC-13 / T1 |
| `OB-11` | Dependency patch P1 (Smithay `leave()` constraint) | OPEN | INC-5 gate |
