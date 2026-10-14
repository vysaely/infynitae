# Permanent Engineering Knowledge System — Provenance Inventory

This document records the exact provenance, source revisions, laboratory logs, test harnesses, and external baselines incorporated into the Knowledge System.

---

## 1. Upstream External Baselines

### 1.1 DriftWM Compositor Baseline
- **Repository:** `https://github.com/malbiruk/driftwm`
- **Author:** Klim Kostiuk
- **License:** **GPL-3.0-or-later** (Strict restriction: zero source code copying permitted into Infynitae Native)
- **Exact Upstream Commit Audited:** [`352333a8fa1b22171492d4b71a54102045c9a19d`](file:///home/vysaely/.gemini/antigravity/scratch/driftwm) (HEAD of `main`, version `0.19.0` + commits)
- **Upstream Smithay Snapshot:** Git revision [`4cf0b62028039661477d482ec4758b687d8f4392`](file:///home/vysaely/.gemini/antigravity/scratch/driftwm/Cargo.toml#L9) (upstream `Smithay/smithay.git` master)
- **Rust Toolchain:** Edition 2024, `rust-version = "1.88"`
- **Key Caveats Document:** [`dev/docs/caveats.md`](file:///home/vysaely/.gemini/antigravity/scratch/driftwm/dev/docs/caveats.md)

### 1.2 Microsoft Windows Graphics Internals Baseline
- **Subject:** Desktop Window Manager (DWM), DirectX Graphics Infrastructure (DXGI), Windows Display Driver Model (WDDM 1.0–3.2)
- **Authoritative Documentation Sources:**
  - Microsoft Learn: *"DXGI Flip Model, DirectFlip, and Independent Flip"*
  - Microsoft Learn: *"Multiplane Overlay Support"* (WDDM DDK)
  - Microsoft Learn: *"Reduce latency with DXGI 1.3 swap chains"* (Waitable swapchains)
  - Microsoft Learn: *"Timeout detection and recovery (TDR)"* (Display Device Technologies)
  - Microsoft Learn: *"Monitored fence synchronization"* (D3D12 / WDDM 2.0)
  - Microsoft Learn: *"Hardware-accelerated GPU scheduling"* (WDDM 2.7+)
  - Microsoft Learn: *"Using dirty rectangles in DXGI 1.2"*

---

## 2. Infynitae Native Commit Provenance

| Commit | Branch / Worktree | Description | Provenance Role |
| :--- | :--- | :--- | :--- |
| `ccb82c5` | `main` | Governance: registry layers C12.F3 and C12.F4, the OB-13 split | Official Ratified Baseline |
| `f57315c` | `hardening/inc-6-scale-policy` | Record single-path P7 correction & failed first rehearsal | Audited Worktree Baseline |
| `f1551d4` | `continuation/inc-7-audit-r2` | Audit finding A-5 on the wire: pointer times go back on presentation change | Audited Continuation Line |
| `07acfeb` | `experimental/inc7-exit-readiness-2026-10-14` | A grant made before first map takes output when element maps (A-3) | Decision Resolution (A-3) |
| `0fe8f06` | `experimental/inc7-exit-readiness-2026-10-14` | Keyboard focus does not move to a window a presentation hides (A-8) | Decision Resolution (A-8) |
| `df72202` | `experimental/inc7-exit-readiness-2026-10-14` | Default placement after fullscreen at commit after exit acknowledged (P9b) | Decision Resolution (P9b) |
| `3b0a3d7` | `experimental/inc7-exit-readiness-2026-10-14` | Popups of a toplevel are stacked newest in front (P10 #3, W-SM-20) | Decision Resolution (W-SM-20) |
| `489180e` | `experimental/inc7-exit-readiness-2026-10-14` | No per-element focus check without a presentation (A-8 perf) | Performance Optimization |
| `e40eb70` | `experimental/combined-perf-audit-2026-10-14` | Synthesised pointer event carries input clock's time, never 0 (A-5) | Architecture Resolution (A-5) |
| `f1ef262` | `perf/inc-6-scale-index` | OB-42 counting-allocator ratchet in Core test; P-1 repair record | Performance Verification (OB-42) |

---

## 3. Laboratory Artifacts & Reports

1. **`driftwm_agy_audit.md`** (Session `c54deef2-d9ed-4960-94c0-add46699170d`, 37.5 KB): Initial comparative audit of DriftWM by AGY.
2. **`INDEPENDENT_REVIEW.md`** (`/home/vysaely/.gemini/antigravity/scratch/INDEPENDENT_REVIEW.md`, 17.2 KB): Independent adversarial review evaluating AGY's findings, methodology, and empirical evidence.
3. **`DISPUTED_FINDINGS.md`** (`/home/vysaely/.gemini/antigravity/scratch/DISPUTED_FINDINGS.md`, 16.7 KB): Detailed breakdown of contradicted, incorrect, and refuted claims with corrective experiments.
4. **`VERIFIED_OPPORTUNITIES.md`** (`/home/vysaely/.gemini/antigravity/scratch/VERIFIED_OPPORTUNITIES.md`, 7.1 KB): Clean-room engineering opportunities and overlooked architectural discoveries.
5. **`FINAL_RECOMMENDATION.md`** (`/home/vysaely/.gemini/antigravity/scratch/FINAL_RECOMMENDATION.md`, 7.8 KB): Ranked implementation roadmap and rejected optimization ledger.
6. **`MICROSOFT_GRAPHICS_ARCHITECTURE_RESEARCH.md`** (`/home/vysaely/.gemini/antigravity/scratch/MICROSOFT_GRAPHICS_ARCHITECTURE_RESEARCH.md`, 41.2 KB): Deep-dive research into DWM, DXGI, WDDM, MPO, DirectFlip, and Linux DRM/KMS equivalents.
