# KNOW-PITFALL-007: Smithay 0.7.0 xdg-shell Limitations Found in INC-7 (W-SM-18, W-SM-19, W-SM-20)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-PITFALL-007 |
| **title** | Smithay 0.7.0 xdg-shell Limitations Found in INC-7 (W-SM-18, W-SM-19, W-SM-20) |
| **subsystem** | Wayland/Popups |
| **source_rev** | vendor/smithay-0.7.0; upstream Smithay 641d2ad; records in FOUNDATION_BLOCKERS (living) on b57c063 / 93c2605 |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | While Smithay is pinned at 0.7.0 (D-B12-PIN). The W-SM records are living entries; their responses are UNRATIFIED with INC-7 |
| **affected_branches** | experimental/inc7-exit-readiness-2026-10-14, integration/inc-7-exit-2026-10-14 |
| **related_invariants** | OB-18, OB-1, X-15, W-SM-6, W-SM-7 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | At the next Smithay pin update: retire W-SM-18 (require code 3); before INC-14: implement the W-SM-19 guard; keep W-SM-20's characterisation test |

---

## Verified Facts
1. **W-SM-18 — error code.** A buffer committed on an xdg surface before a configure is acknowledged is answered with `xdg_surface` error **1** (`not_constructed`) by `ensure_configured`; xdg-shell names **3** (`unconfigured_buffer`). Fixed upstream by commit `641d2ad` ("wayland/shell/xdg: Track configured state through MultiCache last_acked", PR #1817, merged 2025-09-17), which removes `ensure_configured` and checks in pre-commit hooks. No released Smithay contained the fix on 2026-10-14 (latest release v0.7.0, 2025-06-24). The adapter cannot post code 3 itself: Smithay 0.7.0 exposes no `xdg_surface` handle.
2. **W-SM-19 — stale pointer grab.** When a client holding a popup grab dies, the keyboard grab ends at once, but the pointer grab object stays installed until the next pointer event. Observed in the INC-7 exit review: after an offending client holding a two-deep grabbing chain was disconnected, `is_pointer_grabbed()` read `true` until one pointer event, `false` after. No event reaches any client meanwhile.
3. **W-SM-20 — popup stacking.** `PopupManager::popups_for_surface` (and `Window::render_elements`) lists a popup's children oldest first and root popups in creation order, so an older sibling (or an older popup's subtree) is in front of a newer popup. xdg-shell: "A newly created xdg_popup will be stacked on top of all previously created xdg_popup surfaces associated with the same xdg_toplevel." The adapter now orders popup trees by creation number (newest first).

## Unverified Hypotheses
- That upstream's pre-commit check is a drop-in replacement for the adapter's two `ensure_configured` calls is not verified (it will be at the pin update).

## Performance & Correctness Implications
- W-SM-18: cosmetic (the client is disconnected either way); tests accept code 1 or 3.
- W-SM-19: harmless today; anything that decides on the grab state without a pointer event (INC-14 interactive move/resize) would read a stale grab.
- W-SM-20: any future code path rendering or hit-testing through Smithay's `Window` would silently reintroduce the wrong order (OB-26 already forbids `Window::surface_under`).

## Limitations & Counterexamples
- `t1_popup_policy` tolerates W-SM-19 (it records, but does not assert, the grab state before the next pointer event).

## Reproduction Instructions
```bash
cargo test -p infynitae-backend-headless --test t1_popup_policy
cargo test -p infynitae-backend-headless --test t1_inc7_misbehaving
cargo test -p infynitae-backend-headless --test t1_inc7_adversarial a_newer_popup_is_stacked_above_every_older_one
cargo test -p infynitae-backend-headless --test t1_parity ob1_nested_popup_chain_matches_what_is_presented
```

## Links & Citations
- Upstream: https://github.com/Smithay/smithay/commit/641d2ad03742e7627c128e424922dcd2471e37a7 (PR #1817)
- Related Decision: [KNOW-DEC-004: Popup Stacking](../decisions/KNOW-DEC-004-p10-popup-stacking-w-sm-20.md)
- Related Pitfall: [KNOW-PITFALL-003: Bogus Grab Serial Tolerance](KNOW-PITFALL-003-bogus-grab-serial-tolerance.md)
