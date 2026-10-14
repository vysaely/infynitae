# KNOW-RES-003: DriftWM Popup Grabs and Lock Reentrancy Workarounds

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-003 |
| **title** | DriftWM Popup Grabs and Lock Reentrancy Workarounds |
| **subsystem** | Research/DriftWM |
| **source_rev** | 352333a8fa1b22171492d4b71a54102045c9a19d |
| **date** | 2026-10-14 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | Smithay 0.7.x adapter integration safety |
| **affected_branches** | N/A (External Reference) |
| **related_invariants** | W-SM-7, W-SM-15, W-SM-20 |
| **adoption_status** | RESEARCH |
| **next_action** | Enforce scrutinee drop lints in workspace |

---

## Verified Facts
1. **Deferred Pointer Ungrab:** In DriftWM ([`src/state/keyboard_focus.rs:224-239`](file:///home/vysaely/.gemini/antigravity/scratch/driftwm/src/state/keyboard_focus.rs#L224-L239)), `tear_down_popup_grab` defers pointer ungrab to `loop_handle.insert_idle()`. This was necessitated because DriftWM's `PanGrab` (a custom Smithay `PointerGrab`) called focus teardown from within its own button handler while holding `PointerInner`'s non-reentrant mutex.
2. **Smithay Lock Reentrancy:** Calling any helper that queries surface state inside `compositor::with_states()` deadlocks silently because `UserDataMap`'s lock is non-reentrant.
3. **Scrutinee Lock Retention:** Holding `output_state()` across a `match` or `if let` block keeps the mutex guard alive until the end of the block. DriftWM enforces protection via `clippy::significant_drop_in_scrutinee = "warn"`.
4. **Permissive Serial Bug:** DriftWM deliberately ignores invalid grab serials ([`src/tests/popups.rs:107`](file:///home/vysaely/.gemini/antigravity/scratch/driftwm/src/tests/popups.rs#L107)), violating XDG Shell protocol security.

## Unverified Hypotheses
- Hypothesized that Infynitae requires `insert_idle` ungrab; refuted by independent review (Infynitae does not use a Smithay `PointerGrab` for panning and ungrabs via `Inner` handle).

## Performance & Correctness Implications
- Adopting Clippy's `significant_drop_in_scrutinee = "warn"` provides zero-cost compile-time deadlock prevention.

## Limitations & Counterexamples
- Permissive grab serial acceptance allows malicious or unprivileged clients to hijack all pointer input without user interaction.

## Reproduction Instructions
Inspect DriftWM caveats and grab tests:
```bash
view_file /home/vysaely/.gemini/antigravity/scratch/driftwm/dev/docs/caveats.md (lines 15-40)
```

## Links & Citations
- Source Reference: `driftwm/src/state/keyboard_focus.rs`
- Related Pitfall: [KNOW-PITFALL-003: Bogus Grab Serial Tolerance](../pitfalls/KNOW-PITFALL-003-bogus-grab-serial-tolerance.md)
- Related Decision: [KNOW-DEC-004: Popup Stacking Correction](../decisions/KNOW-DEC-004-p10-popup-stacking-w-sm-20.md)
