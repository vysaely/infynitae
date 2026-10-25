# Unmap of a fullscreen element (registry layer C12.F10; INC-7 residual RC-1) — record

Status: evidence and provenance for FOUNDATION_ARCHITECTURE §C12.F10. Adopting this layer does not
ratify INC-7.

## The contradiction

At `80ead2a`, M9 had one row: "Fullscreen{O} | `unset_fullscreen` / unmap | Windowed | configure with
the saved size, if available; …". For unmap, no such configure exists.

- **Sealed source:** audit/1R/STATE_MACHINES_1R M9' (`84ce620`) has two rows: "`unset_fullscreen`
  → configure with the saved size; presentation ends now" and "unmap → state dropped (spec)". The D7
  consolidation of the living machines (`c0a1cee`) merged them, and C12.F6 qualified the merged row
  without separating it.
- **Implementation (unchanged since `93c2605`):** the null-buffer commit drops the state through
  `drop_fullscreen` (grant, pre-map request, size on offer, postponed placement) and emits no
  fullscreen configure.

## Evidence

| Observation | Source |
|---|---|
| Core emits no `ToplevelConfigureSize` at unmap; state empty; remap configure state is the default (not fullscreen) | T0 `fullscreen_ends_cleanly_on_unmap_destroy_and_output_changes` |
| on the wire: one configure for the re-initial commit, not fullscreen; no state survives | T1 `unmap_remap_and_destruction_leave_no_fullscreen_state` |
| seeded: "fullscreen state survives an unmap", "a size on offer survives the unmap" | `tools/verify-test-efficacy` (detected) |
| wire probe (scratch, not committed): W focused at unmap → exactly one configure at unmap, `0×0`, no states (the M11 target-loss `activated` configure); W unfocused → none; re-initial commit → one configure `0×0`, not fullscreen | `~/Infynitae-INC7-EXIT-PREPARATION-2026-10-14/evidence/rc1-probe.log` |
| Core probe (scratch): effects for W at unmap: `[ToplevelConfigure(W)]` with W focused (from `FocusModel::target_lost`), `[]` without | same |

## What the layer does

It adds one sentence to INV-PRES-1 and splits M9's row into `unset_fullscreen` and unmap, as 1R did.
It changes no behaviour and no code.
