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

---

## 4. Reconciliation Additions (2026-10-14, main `f323c2d`)

### 4.1 Archived originals
The six reports in §3 live in AGY's scratch directory, outside version control. Byte-identical copies, hashed, are in `~/Infynitae-KNOWLEDGE-SOURCES-2026-10-14/` (`SHA256SUMS`, `README.md`). They are kept out of the repository on purpose: `driftwm_agy_audit.md` quotes DriftWM (GPL-3.0-or-later) source excerpts, and the reports carry claims later corrected (CONTRADICTIONS_AND_SUPERSEDED).

| Report | SHA-256 | Author / trajectory |
| :--- | :--- | :--- |
| `driftwm_agy_audit.md` | `40972b9caef5991d690aef6e5ecf0efe6643987687f14828e70ade67a18bf3cd` | AGY `c54deef2-d9ed-4960-94c0-add46699170d` |
| `INDEPENDENT_REVIEW.md` | `adcdaac78b22755deeb3484c7fc845deb95ed2c656eb315dede7eaf806d9700f` | AGY `822720a8-8ac6-4db8-b380-1d3a7433c018` |
| `DISPUTED_FINDINGS.md` | `d8119de5fe56d63bdd7c8397bf97f7c924a8dd1ba3b9260cafe63588fecfe0ba` | same |
| `VERIFIED_OPPORTUNITIES.md` | `58a49f56b4945c6527ba670366a2f47f9e79f0e8798495966371f0088811fe13` | same |
| `FINAL_RECOMMENDATION.md` | `99993b7664c0ab71ccb5ed1becf442ac71abb2030ac330353e1a1184fdf18b86` | same |
| `MICROSOFT_GRAPHICS_ARCHITECTURE_RESEARCH.md` | `a031a2cf721e6430c62553c5c233b3d7019db931a26f7427c4667595e411d36b` | same |

Trajectory `822720a8` also wrote the first version of this knowledge system (`8119726`). The second knowledge commit (`8e91f42`) is Claude's INC-7 exit preparation. On the integration branch both are cherry-picked onto `f323c2d` with `-x`, so their messages name the original commits.

### 4.2 Raw measurement
`pull_cost_bench` (DriftWM `352333a`, release, one run, 2026-10-14 in trajectory `822720a8`): "pull over bare canvas: 9.92µs; pull over a window: 3.755µs; one real absolute motion over bare canvas: 15.804µs". Transcript path in the archive README.

### 4.3 Where each record's best version lives
`docs/permanent-knowledge-system` (`8119726`) is an ancestor of `knowledge/inc7-exit-preparation-2026-10-14` (`8e91f42`); no record differs between them except those the second commit corrected, so the second is the more accurate of the two for every record. Both are based on `ccb82c5`. `knowledge/integration-2026-10-14` carries both onto `f323c2d` and adds this reconciliation.

### 4.4 INC-7 ratification provenance (main)
| Commit | What |
| :--- | :--- |
| `d70c3be`, `b1aa758`, `0f45cfd`, `334b9b3`, `23860b3`, `d350ed2`, `2aa8d65` | A-5, A-3, A-8, P9b, X-15, control re-anchor, A-8 fast path (ported from the source line; KNOW-DEC-005) |
| `3cf85f5`, `32bfbfb`, `37bed72`, `a9ec294` | Layers C12.F5, C12.F6, C12.F7, C12.F8 |
| `a43a149` | Layer C12.F10 (RC-1) |
| `39ad614` | INC-7 ratification (OB-1, OB-18, OB-32 closed) |
| `f323c2d` | CHECKPOINT: residuals are tracked follow-up work; = origin/main |

Packages: `~/Infynitae-INC7-EXIT-PREPARATION-2026-10-14/` (exit decision, residual register R-01…R-15, ratification and publication audit), `~/Infynitae-INC7-EXIT-REVIEW-2026-10-14/` (final governance review, residual risk register).

### 4.5 Not inspected
- ~~The old Arch Linux partition (historical Claude/AGY sessions; Hyprland-era Infynitae). Not mounted; mounting needs root. It may hold earlier DriftWM camera/viewport research (gap G-01).~~ **2026-10-15:** no longer present on this machine (its disk now holds Windows) and predates DriftWM's first appearance (2026-10-14); it held no DriftWM research. Details in the ledger, G-01. Nothing was mounted.
- Microsoft Learn pages themselves (KNOW-RES-008).
