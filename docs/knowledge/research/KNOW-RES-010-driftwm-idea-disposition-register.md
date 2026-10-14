# KNOW-RES-010: DriftWM Ideas — Disposition Register

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-010 |
| **title** | DriftWM Ideas — Disposition Register |
| **subsystem** | Research/DriftWM |
| **source_rev** | DriftWM 352333a8fa1b22171492d4b71a54102045c9a19d; driftwm_agy_audit.md §9–§14, VERIFIED_OPPORTUNITIES.md, FINAL_RECOMMENDATION.md; Infynitae main f323c2d |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | Anyone considering a DriftWM idea; INC-6, INC-8, INC-14, DRM/KMS backend |
| **affected_branches** | main (f323c2d) for the "Infynitae today" column |
| **related_invariants** | INV-PRES-1, INV-INPUT-8, INV-FRAME-1, INV-CAMERA-1, OB-26, OB-42 |
| **adoption_status** | RESEARCH |
| **next_action** | Re-check a row's status before acting on it; add a row when a new DriftWM idea is studied |

---

## Verified Facts
1. **Every DriftWM idea found in the recovered research, with its disposition.** DriftWM is GPL-3.0-or-later: adoption means a clean-room reimplementation of the concept, never its code.
   | Idea (DriftWM location as cited) | Disposition | Infynitae today / owner | Record |
   | :--- | :--- | :--- | :--- |
   | Own `Stage` instead of Smithay `Space` for layout and z-order | Already stronger in Infynitae | Core owns all window state with no Smithay types | KNOW-RES-001 |
   | `restore_size` decoupled from live geometry (A-1) | Implemented (equivalent) | `77cde7f` on main | KNOW-RES-002 |
   | Queue pre-map fullscreen until the first buffer (A-3) | Implemented (Infynitae's own design, ratified) | C12.F6, `b1aa758` | KNOW-DEC-001 |
   | Fullscreen by relocating the window, zoom 1.0 | Rejected | Projection override, anchors untouched | KNOW-PITFALL-001 |
   | Adapter wall-clock pointer times (A-5) | Rejected | Core `InputClock`, ratified | KNOW-PITFALL-004, KNOW-ARCH-002 |
   | No focus check behind fullscreen | Not copied; Infynitae refuses focus moves to hidden windows | C12.F7; retention residual R-01 | KNOW-DEC-002 |
   | Per-iteration pointer-focus pull | Rejected on design grounds (not measured, X-08) | Reconciliation at Core chokepoints | KNOW-PITFALL-002, KNOW-RES-009 |
   | Accept invalid popup-grab serials | Rejected | W-SM-7 rejection | KNOW-PITFALL-003 |
   | Defer pointer ungrab to `insert_idle` | Not needed (deadlock was DriftWM's own `PanGrab`) | Ungrab through `&mut Inner` | KNOW-RES-003 |
   | Smithay `PopupManager` order for popups | Not copied; Smithay's order is wrong (W-SM-20) | Newest first, C12.F8 | KNOW-DEC-004 |
   | `get_popup_toplevel_coords` for nested popups (A-2) | Equivalent | Own chain walk `8db8080` | — |
   | 995 ms off-screen frame-callback throttle | Useful, only gated (never inventing callbacks, INV-FRAME-1) | None yet; R-10 → **INC-8** | KNOW-PITFALL-006 |
   | Window membership (`enter`/`leave`) from one function | Implemented | Core membership; INV-COORD-6 | KNOW-RES-009 |
   | Camera animation: input tracks the live viewport | Useful later | Any time-driven camera step needs its own proof (INV-CAMERA-1) | KNOW-RES-009 |
   | Clippy `significant_drop_in_scrutinee = "warn"` | Useful, **not adopted** (workspace `Cargo.toml` has no lint table on main) | Candidate tooling change | KNOW-RES-003 |
   | Never call a Smithay helper inside `compositor::with_states` | Useful, **not audited** (8 `with_states` call sites in `infynitae-wayland/src` on main) | Candidate review item | KNOW-RES-003 |
   | Scrub `PopupKeyboardGrab` left after a popup unmaps | Related to W-SM-19 | **INC-14 entry condition** (R-07) | KNOW-PITFALL-007 |
   | `ConstraintSnapshot`: copy pointer-constraint data out under lock | Useful pattern | Constraints are Core-owned (INV-INPUT-5); check at INC-14 | — |
   | Initial configure sent once, after window rules are resolved ("initial burst is a single configure", for SDL2/SCTK) | Useful; was wrongly presented by AGY as P9b (C-04) | Core answers each request with exactly one configure (M2/M9); initial-burst behaviour with toolkits not studied | KNOW-DEC-003 |
   | Primary/cursor plane direct scanout (`ALLOW_PRIMARY_PLANE_SCANOUT_ANY`) | Reference for later | **DRM/KMS backend** | KNOW-ROAD-001, KNOW-EXP-004 |
   | CPU fence wait before a flip when the driver cannot take `IN_FENCE_FD` (`wait_for_fence`, `needs_sync()`; NVIDIA) | Reference for later | **DRM/KMS backend** | KNOW-ROAD-002 |
   | Optional Tracy profiling feature | Optional; not planned | — | — |
   | `xwayland-satellite` out-of-process X11 | Deferred | X11 milestone | — |
   | CSD "tiled" hack for corner clipping | Not needed | — | — |
   | Per-frame `Vec` allocations in composition | Rejected as a target | OB-42 (open, INC-6 line) | KNOW-EXP-002 |
   | *Added 2026-10-15 from the source study (KNOW-RES-011):* | | | |
   | Camera momentum / animated zoom, with the pointer kept screen-fixed by warping its canvas location | Product candidate, not specified | Would need an owner decision and per-step authorization (INV-CAMERA-1) | KNOW-RES-011 |
   | Zoom-to-fit with dynamic minimum zoom from the windows' bounding box | Product candidate; viewport-fit specified, not implemented | Viewport-fit is specified as a one-shot authorized action (INV-CAMERA-5); no code on main `c6d47c9`; no dynamic minimum | KNOW-RES-011 |
   | `auto` placement next to the focused window's cluster | Product candidate, not specified | F-PLACE-1 centres on the camera; any cluster placement is a product decision | KNOW-RES-011 |
   | Camera as `f64` top-left, accumulated per pan event | Rejected (by existing design) | Q16 centre, re-derived from the gesture origin (INV-COORD-3) | KNOW-RES-011 |
   | Membership from the zoom-1 output rect while culling uses the zoomed rect | Rejected (divergence) | One `View` for render, membership and hit-test (F-MEMBER-1, F-HIT-1) | KNOW-RES-011 |
   | Per-iteration focus pull over all stage entries | Rejected on design grounds (unchanged; cost traced as O(W) per pick, not measured) | Core chokepoints | KNOW-RES-011, KNOW-PITFALL-002 |

## Unverified Hypotheses
- The rows marked "Useful" have not been prototyped in Infynitae; their value is argued, not measured.

## Performance & Correctness Implications
- Before importing a DriftWM idea, check this table and the pitfall records: four of the audit's "DriftWM stronger" recommendations (§9) turned out wrong or unnecessary for Infynitae.

## Limitations & Counterexamples
- Coverage is limited to what the recovered reports mention; DriftWM is about 109,000 lines and was not studied as a whole (gap G-01). *2026-10-15:* camera, viewport, culling, hit-testing and placement were then traced in source (KNOW-RES-011); the rest of DriftWM remains unstudied.

## Reproduction Instructions
```bash
grep -c . ~/Infynitae-KNOWLEDGE-SOURCES-2026-10-14/agy-scratch/driftwm_agy_audit.md
git show main:Cargo.toml | grep -n lints || echo "no lint table"
git grep -c with_states main -- crates/infynitae-wayland/src
```

## Links & Citations
- [KNOW-RES-011](KNOW-RES-011-driftwm-camera-viewport-source-study.md), [KNOW-RES-001](KNOW-RES-001-driftwm-state-ownership.md), [KNOW-RES-002](KNOW-RES-002-driftwm-fullscreen-lifecycle.md), [KNOW-RES-003](KNOW-RES-003-driftwm-popups-and-locks.md), [KNOW-RES-009](KNOW-RES-009-driftwm-camera-viewport-and-hit-testing.md)
- [KNOW-ROAD-001](../roadmap/KNOW-ROAD-001-phase-g-hardware-drm-backend.md), [KNOW-ROAD-002](../roadmap/KNOW-ROAD-002-explicit-timeline-sync-syncobj.md), [KNOW-ROAD-005](../roadmap/KNOW-ROAD-005-inc7-follow-up-work.md)
