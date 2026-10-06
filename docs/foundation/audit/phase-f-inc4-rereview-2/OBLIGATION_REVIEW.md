# Third Review — Obligations, Adjudications, Safety

| OB | Disposition | Reason |
|---|---|---|
| OB-4 | **OPEN** | dismissal before camera change, no pointer events, keyboard coherent — met; stale pointer grab after dismissal (R2-05); dismissed popups leave input holes (R2-01) |
| OB-15 | **OPEN** pending R2-04 (repeat path leaves stuck modifiers) — output-removal part now met |
| OB-18 (popup part) | **CLOSABLE at ratification** — refusal semantics met (PF-WSM7, atk11, M17); W-SM-6 part INC-7 |
| OB-30 | **OPEN** | PF-1…7 pass; NEG-1/NEG-2/OBS-1 pass under probe features but no gate executes them unmutated; depth-3 preemption leaves the pointer grab (R2-05) |
| OB-21 | **OPEN** — INC-16 / T4; physical libinput requires owner authorization |

Owner adjudications: INC4R-10 behaviour matches (doc residual R2-09); INC4R-11 not implemented (R2-06); INC4R-12 implemented.

Safety: production implementation untouched (all mutations in disposable worktrees, restored; repo shows only this untracked dossier). No live compositor, Hyprland/RustDesk, networking, DRM/KMS, VT, boot, power, suspend or package action; builds throttled (`CARGO_BUILD_JOBS=4`). Old Arch partition untouched. INC-5 not started.
