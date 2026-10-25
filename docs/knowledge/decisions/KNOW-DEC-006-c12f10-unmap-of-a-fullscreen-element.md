# KNOW-DEC-006: Unmap of a Fullscreen Element (C12.F10, INC-7 Residual RC-1)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-DEC-006 |
| **title** | Unmap of a Fullscreen Element (C12.F10, INC-7 Residual RC-1) |
| **subsystem** | Core/Presentation, Governance |
| **source_rev** | a43a149 (layer C12.F10), ratified in 39ad614; implementation unchanged since 93c2605 |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | Fullscreen lifecycle on main; anyone editing M9 or INV-PRES-1 |
| **affected_branches** | main (f323c2d) |
| **related_invariants** | INV-PRES-1, M9, M11, C12.F6, C12.F10 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | None; re-check if M9 is consolidated again |

---

## Verified Facts
1. **The contradiction.** At `80ead2a`, living state machine M9 had one merged row "Fullscreen{O} | `unset_fullscreen` / unmap | Windowed | configure with the saved size, if available". For unmap no such configure exists. The sealed 1R machine (`84ce620`) had two rows; the D7 consolidation (`c0a1cee`) merged them, a transcription error found in the INC-7 exit review (RC-1).
2. **The layer.** C12.F10 adds one sentence to INV-PRES-1 and splits M9's row into `unset_fullscreen` (configure with the saved size; the presentation ends now) and unmap (state dropped). It changes no behaviour and no code. Number F9 stays reserved for the INC-6 allocator layer; the checker orders layers by adoption order.
3. **Behaviour (unchanged since `93c2605`).** A null-buffer commit drops all fullscreen state through `drop_fullscreen` (grant, pre-map request, size on offer, postponed placement) and emits no fullscreen configure. If the element was focused, the only configure at unmap is M11's target-loss `activated` configure (`0×0`, no states); a re-initial commit gets one configure, not fullscreen.
4. **Evidence.** T0 `fullscreen_ends_cleanly_on_unmap_destroy_and_output_changes`; T1 `unmap_remap_and_destruction_leave_no_fullscreen_state`; seeded controls "fullscreen state survives an unmap" and "a size on offer survives the unmap" detected.
5. **Status.** Adopted by owner approval and ratified with INC-7 on 2026-10-14.

## Unverified Hypotheses
- None.

## Performance & Correctness Implications
- Consolidating sealed machines can silently merge rows with different outcomes; compare each consolidated row with its sealed sources.

## Limitations & Counterexamples
- The wire probe that observed the configures was a scratch program, not committed (log in the preparation package).

## Reproduction Instructions
```bash
cargo test -p infynitae-core --lib fullscreen_ends_cleanly_on_unmap_destroy_and_output_changes
cargo test -p infynitae-backend-headless --test t1_fullscreen unmap_remap_and_destruction_leave_no_fullscreen_state
```
Record: `docs/archive/evidence/phase-f/fullscreen-unmap-rc1/AMENDMENT_RECORD.md`; probe log `~/Infynitae-INC7-EXIT-PREPARATION-2026-10-14/evidence/rc1-probe.log`.

## Links & Citations
- Related: [KNOW-DEC-001](KNOW-DEC-001-a3-premap-fullscreen-ownership.md), [KNOW-ROAD-005](../roadmap/KNOW-ROAD-005-inc7-follow-up-work.md)
