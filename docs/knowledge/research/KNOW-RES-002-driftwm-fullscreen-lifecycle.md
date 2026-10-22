# KNOW-RES-002: DriftWM Fullscreen Lifecycle and Restore Sizing

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-002 |
| **title** | DriftWM Fullscreen Lifecycle and Restore Sizing |
| **subsystem** | Research/DriftWM |
| **source_rev** | 352333a8fa1b22171492d4b71a54102045c9a19d |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | Comparison with Infynitae Finding A-1 and A-3 |
| **affected_branches** | N/A (External Reference) |
| **related_invariants** | INV-PRES-1, M9 |
| **adoption_status** | RESEARCH |
| **next_action** | Track pre-map fullscreen queueing in Core |

---

## Verified Facts
1. **Saved Restore Size:** DriftWM maintains an independent `restore_size: Option<Size<i32, Logical>>` inside `Entry<W>` ([`src/stage/mod.rs:100`](file:///home/<user>/.gemini/antigravity/scratch/driftwm/src/stage/mod.rs#L100)). It updates this value only during interactive pointer resizes, protecting against A-1 size loss when exiting fullscreen.
2. **Pre-Map Queueing:** DriftWM queues pre-map fullscreen requests in `pending_fullscreen` ([`src/handlers/xdg_shell.rs:173`](file:///home/<user>/.gemini/antigravity/scratch/driftwm/src/handlers/xdg_shell.rs#L173)), applying them only upon the first buffer commit.
3. **Physical Relocation Flaw:** When entering fullscreen, DriftWM teleports the window to `camera_i32` in canvas space and forces zoom to 1.0. This physical relocation requires complex recentering logic (`pending_recenter`) when exiting fullscreen.

## Unverified Hypotheses
- Hypothesized that DriftWM's `pending_recenter` drops reflow anchors when a client acks the exit configure but keeps its pre-exit size; verified in DriftWM caveats documentation line 113.

## Performance & Correctness Implications
- Infynitae's camera-independent projection view (`KNOW-ARCH-001`) completely avoids DriftWM's physical relocation vulnerabilities.
- Infynitae implemented A-3 pre-map protection in Core commit [`07acfeb`](file:///home/<user>/infynitae-native).

## Limitations & Counterexamples
- DriftWM does not check if an output is fullscreen when assigning keyboard focus (`src/state/keyboard_focus.rs:106`), permitting focus on invisible windows.

## Reproduction Instructions
Inspect DriftWM fullscreen exit test:
```bash
view_file /home/<user>/.gemini/antigravity/scratch/driftwm/src/tests/configure_sequences.rs (lines 2773-2840)
```

## Links & Citations
- Upstream Code: `driftwm/src/state/fullscreen.rs`
- Related Architecture: [KNOW-ARCH-001: Camera-Independent Fullscreen Presentation Projection](../architecture/KNOW-ARCH-001-camera-projection-independence.md)
- Related Pitfall: [KNOW-PITFALL-001: Physical Canvas Fullscreen Relocation](../pitfalls/KNOW-PITFALL-001-physical-canvas-fullscreen-relocation.md)
