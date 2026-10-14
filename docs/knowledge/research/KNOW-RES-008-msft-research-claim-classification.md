# KNOW-RES-008: Microsoft Graphics Research — Claim Classification and Transfer Limits

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-008 |
| **title** | Microsoft Graphics Research — Claim Classification and Transfer Limits |
| **subsystem** | Research/MicrosoftGraphics |
| **source_rev** | MICROSOFT_GRAPHICS_ARCHITECTURE_RESEARCH.md (sha256 a031a2cf…a18bf3cd, AGY trajectory 822720a8); classified against main f323c2d |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | Reading guide for KNOW-RES-004..007 and KNOW-ROAD-001..004; input to INC-8 and the DRM/KMS backend |
| **affected_branches** | N/A (Comparative Research) |
| **related_invariants** | INV-FRAME-1, INV-PRES-1, INV-INPUT-5, INV-CAMERA-4 |
| **adoption_status** | RESEARCH |
| **next_action** | Before citing any row marked MS-DOC as fact in a design, open the named Microsoft Learn page and record its URL here |

---

## Verified Facts
1. **What the source is.** One 41 KB report written by AGY on 2026-10-14 (archived byte-identically in `~/Infynitae-KNOWLEDGE-SOURCES-2026-10-14/`). It cites Microsoft Learn page *titles* only, without URLs, and no recovered session fetched any Microsoft page. KNOW-RES-004..007 condensed four of its seven sections and labelled the Microsoft statements "Verified Facts"; this record replaces that label with the classification below. KNOW-RES-004..007 are kept unchanged for provenance.
2. **Classes used here.**
   - **MS-DOC** — a statement of documented Microsoft behaviour, as the source reports it. Not re-checked against Microsoft documentation in any session.
   - **INTERP** — the source's interpretation (an inferred internal, a "parity" claim, a magnitude).
   - **APPLY** — a proposed application to Infynitae. A candidate, not a requirement; only `docs/foundation/` can make it one.
   - **WIN-ONLY** — a Windows mechanism with no direct Linux/Wayland transfer.
3. **Presentation and scheduling (source §1, §2).**
   | Claim | Class |
   | :--- | :--- |
   | DWM is a retained-mode compositor; apps render to redirected surfaces composed each vblank | MS-DOC |
   | Flip model hands buffers by reference instead of the Blt-model copy | MS-DOC |
   | DirectFlip (Win 8 / DXGI 1.2) skips the composition blit when size, format and overlap conditions hold | MS-DOC |
   | Independent Flip lets the app's swapchain present without DWM composing it | MS-DOC |
   | "DWM suspends its composition thread entirely" under IFlip | INTERP (stronger than the documented behaviour) |
   | DWM leaves IFlip "within 1 frame" with a cross-fade or atomic plane swap | INTERP (the source itself calls it inferred) |
   | MPO keeps a game on its own plane while other UI is on other planes | MS-DOC |
   | DRM/KMS atomic planes are the Linux counterpart of DirectFlip/MPO | INTERP (counterpart, not "exact parity": the Windows negotiation is in DWM and the driver, a Linux compositor does it itself with `TEST_ONLY` commits) |
   | DXGI 1.3 waitable object lets the app wait until the queue has room; latency can be set to 1 | MS-DOC |
   | Default queueing "adds 50–100 ms of input lag" | INTERP (magnitude unsourced; depends on refresh rate and frame time) |
   | Presentation Manager / present-at-time scheduling (Windows 11) | MS-DOC; Linux counterpart `wp_presentation_time` plus client timing extensions, not a compositor scheduling API |
   | Frame callbacks + `wp_presentation_time` are the Wayland counterpart of the waitable object | INTERP (client-paced by compositor feedback; there is no queue-depth object) |
4. **GPU synchronization (source §6).**
   | Claim | Class |
   | :--- | :--- |
   | WDDM 2.0 monitored fences are monotonically increasing 64-bit values | MS-DOC |
   | Shared and signalled "without kernel transitions" | INTERP |
   | `drm_syncobj` timelines and `linux-drm-syncobj-v1` give "exact parity" | INTERP (same monotonic-point model; different wait-before-signal and release semantics) |
   | Requires kernel ≥ 6.8 and Mesa ≥ 24.1 | INTERP (version floors unsourced; re-check at the DRM/KMS milestone) |
   | Implicit sync causes "deadlock hazards" and priority inversion | INTERP |
5. **Scheduling, preemption, recovery (source §3, §7).**
   | Claim | Class |
   | :--- | :--- |
   | WDDM preemption granularity grew from DMA-buffer (1.0) to mid-buffer/instruction (2.0) | MS-DOC |
   | DWM composes at realtime GPU priority | MS-DOC |
   | HAGS (WDDM 2.7) moves scheduling onto a GPU scheduling processor | MS-DOC; the source's "RISC-V / microcode" detail is INTERP |
   | TDR: default `TdrDelay` 2 s, engine/adapter reset, `DXGI_ERROR_DEVICE_RESET`/`REMOVED`, DWM recreates its device | MS-DOC |
   | DWM paints cached fallback brushes during a reset | INTERP (source says inferred) |
   | Hung-window ghosting replaces a non-responding window's picture | MS-DOC; the mechanism (private texture clone) is INTERP |
   | CCD applies display topology atomically | MS-DOC |
6. **Damage and input (source §4, §5).**
   | Claim | Class |
   | :--- | :--- |
   | `Present1` dirty rects and scroll rect; DWM composes only damaged regions | MS-DOC |
   | Raw Input (`WM_INPUT`) delivers uncoalesced HID input; `WM_MOUSEMOVE` coalesces | MS-DOC |
   | `GetMessageTime` is a 32-bit ms tick; QPC is high resolution | MS-DOC |
   | Windows "suffers time-stamp inversion" mixing the two | INTERP (unsourced) |
7. **Statements about Infynitae in the source, checked against main `f323c2d`.**
   - "INV-CAMERA-4 → no spurious damage on fullscreen outputs": INV-CAMERA-4 is about configures and pointer events from a pan, not damage. No damage claim is sealed. Unsupported.
   - "Infynitae tracks damage deterministically per surface tree node": Core has no damage model; frames are produced "through the adapter's damage trackers" (Smithay's, `infynitae-backend-headless/src/lib.rs` module doc) and the winit backend submits Smithay's damage rects. Unsupported as stated.
   - "Allocation ratchets 15/9/4": real, but on the INC-6 line (`f1ef262`), not on main.
   - "Hidden or off-screen windows get no frame callbacks until uncovered": true on main (INC-7 residual R-10, audit A-7, owned by INC-8).
   - "INC-8 must incorporate a gated heartbeat": APPLY, not a requirement. INV-FRAME-1 ("only completes frame callbacks clients requested; never invents them") governs; a throttled completion of a requested callback is compatible with it, an unrequested one is not.
8. **Mislabelled "Windows anti-patterns".** The source's Class 4 ("REJECT: wall-clock adapter timestamps, global focus pull loops, permissive grab serials") are DriftWM/AGY findings, not Windows mechanisms; only "Blt-model copy presentation" is Windows. Their rejection records are KNOW-PITFALL-002..004.
9. **WIN-ONLY (do not turn into Linux requirements).** DWM's process-level composition and its IFlip negotiation; DXGI swapchain objects and the waitable handle; WDDM's kernel GPU scheduler policy, VidMm residency and HAGS (Linux has `drm_sched`, TTM/GEM in the kernel; a compositor does not own them); `TdrDelay` registry tuning; User32 ghost windows; CCD. Their *lessons* transfer (present without composing when possible; bound queue depth; explicit timeline sync; survive device loss; keep the desktop responsive when a client hangs); their mechanisms do not.

## Unverified Hypotheses
- Every APPLY row: plane scanout for fullscreen presentation (KNOW-ROAD-001, KNOW-EXP-004), `linux-drm-syncobj-v1` (KNOW-ROAD-002), VRR (KNOW-ROAD-003), `FB_DAMAGE_CLIPS` (KNOW-ROAD-004), high-priority render context, context-loss recovery (KNOW-RES-007), 8 kHz motion micro-coalescing, dimming a client that fails `xdg_wm_base.ping`. None was prototyped or measured. The source's magnitudes (">85 % PSR residency", "up to 70 % dispatch reduction", "<150 µs at 8 kHz", "restore within 500 ms") are targets, not results.
- Micro-coalescing motion would interact with INV-COORD-3 (pan quantised once per update) and with relative-pointer clients that need every delta; untested.

## Performance & Correctness Implications
- Use the Microsoft research as a list of questions for the DRM/KMS backend and INC-8, not as a list of facts about Linux drivers.
- Sources of the four condensed records (KNOW-RES-004..007) should be read with the classes above: their "Verified Facts" sections mix MS-DOC and INTERP.

## Limitations & Counterexamples
- The classification is by reading, not by re-fetching Microsoft documentation (the owner asked for no new large research). An MS-DOC row may still be wrong in detail.
- Sections of the source that no other record carries are kept only here and in the archived file: §3 preemption/priority/HAGS/VidMm, §4 dirty rectangles, §5 input delivery, §7 hung clients, ghosting and CCD, §2 Presentation Manager.

## Reproduction Instructions
```bash
sha256sum ~/Infynitae-KNOWLEDGE-SOURCES-2026-10-14/agy-scratch/MICROSOFT_GRAPHICS_ARCHITECTURE_RESEARCH.md
# a031a2cf721e6430c62553c5c233b3d7019db931a26f7427c4667595e411d36b
git show main:docs/foundation/FOUNDATION_INVARIANTS.md | grep -A1 '^### INV-FRAME-1'
```

## Links & Citations
- Condensed records: [KNOW-RES-004](KNOW-RES-004-msft-directflip-independent-flip-mpo.md), [KNOW-RES-005](KNOW-RES-005-msft-dxgi-waitable-frame-pacing.md), [KNOW-RES-006](KNOW-RES-006-msft-wddm-fences-timeline-sync.md), [KNOW-RES-007](KNOW-RES-007-msft-tdr-device-loss-recovery.md)
- Roadmap: [KNOW-ROAD-001](../roadmap/KNOW-ROAD-001-phase-g-hardware-drm-backend.md), [KNOW-ROAD-002](../roadmap/KNOW-ROAD-002-explicit-timeline-sync-syncobj.md), [KNOW-ROAD-003](../roadmap/KNOW-ROAD-003-vrr-adaptive-sync-pacing.md), [KNOW-ROAD-004](../roadmap/KNOW-ROAD-004-fb-damage-clips-psr.md)
- Corrections: [CONTRADICTIONS_AND_SUPERSEDED.md](../CONTRADICTIONS_AND_SUPERSEDED.md) X-10, X-11
