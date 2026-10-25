# Phase D corrective checkpoint 2R — authority, state-machine and matrix deltas

This document records changes against [../1R/MATRICES_1R.md](../1R/MATRICES_1R.md), [../1R/STATE_MACHINES_1R.md](../1R/STATE_MACHINES_1R.md) and the baseline D2/D3/D4. Rows not listed are unchanged. Canonical wording: FOUNDATION_ARCHITECTURE §C12.R2.

## D3 deltas (authority)

| # | State | 2R row | Finding |
|---|---|---|---|
| 10/11 | Camera centre / zoom | Legal writers: `navigation` only, each write consuming a fresh `Authorized` proof minted in one of three authority entries (`InputPort`, `ShellPort`, `OutputLifecycleToken`). The output-lifecycle entries may **create, bind or unbind only**; they never set or alter a centre or zoom. **Forbidden:** protocol handlers, client commits, timers, the backend's descriptor contents (no coordinate field), any public restore API. | PD-F2-18 |
| 12 | Camera ↔ output binding | Owner: `navigation` (camera set), changed only inside `output_connected`/`output_disconnected` with the token. Precedence on connect: remembered session camera, then persisted lookup, then internal default. Reconstruction: re-enumeration through the same entry. | PD-F2-18 |
| 39 | Persistence | Persisted camera entries are a **passive read-only lookup** owned by Core after `Core::new`. They are consulted only inside `output_connected`; there is no public restore API. | PD-F2-18 |
| 41 (new) | `OutputLifecycleToken` | Class A (authority token). Minted by `Core::new`. Holder: the backend output/display source, exclusively. Unforgeable, not Clone, `!Send`, bound to its Core. **Forbidden holders:** protocol state (E0277), the adapter crate (OB-19 source rule), the shell. | PD-F2-18 |
| 22 | `ScaleState` epoch | `epoch{open, kind ∈ {CameraPan, CameraZoom, WindowMove, WindowResize, ShellStream}, direction, deadline}` | PD-F2-19 |

**Authority searches re-run:**
- **S1 multiple writers:** camera creation and binding now have one authorized path. None open.
- **S2 cycles:** none.
- **S5 protocol objects:** none.
- **S6 backend durable truth:** the backend supplies hardware facts only and cannot choose camera state (probe p8, E0560). None open.

## State-machine deltas

**M4'' Output lifecycle** (replaces the authority column of M4):

| From | Trigger | To | Effect |
|---|---|---|---|
| Absent | `output_connected(token, descriptor)` (startup enumeration or runtime hotplug: same entry) | Enabled | global created; camera bound by precedence (remembered, then persisted, then default); Views, membership, ladder; ScaleInteractionEpoch settlement |
| Enabled | mode/scale/transform change | Enabled | the same token-carrying entry (descriptor update); no camera value change |
| Enabled | `output_disconnected(token, output)` | Removed | INV-OUTPUT-1 sequence; camera unbound, value kept |

**M5'' Camera lifecycle** (restores the creation row that M5' dropped):

| From | Trigger | To | Effect |
|---|---|---|---|
| — | `output_connected`, no remembered camera for the key | Bound | new camera, seeded from the persisted lookup or the internal default; one proof |
| Unbound | `output_connected`, same `OutputKey` | Bound | re-bound, value unchanged; one proof |
| Bound | `output_disconnected` of its last bound output | Unbound | value kept; one proof |
| Bound | navigation via `InputPort` / `ShellPort` | Bound | as M5' |
| any | protocol event, client commit, descriptor contents | unchanged | — (probes p6–p9, `probe_protocol_creates_camera`) |

**M7'' ScaleInteractionEpoch:** M7' with `kind` extended to WindowMove and WindowResize (opened at interaction acquisition, settled at the initiator release) and ShellStream covering shell `MoveElement` streams. The settlement timeout applies to discrete steps.

## D4 delta (coordinates)

**X28 (new):** persisted camera entry → camera. The camera is seeded from `(OutputKey, centre Q16 raw, zoom)` on first connect of that key in a session. The values are exact (as X24) and are not used when the camera is remembered this session. **No caller-supplied coordinate exists on any output path** (probe p8).
