# KNOW-DEC-004: Popup Stacking Order and Smithay Traversal Defect (W-SM-20)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-DEC-004 |
| **title** | Popup Stacking Order and Smithay Traversal Defect (W-SM-20) |
| **subsystem** | Wayland/Popups |
| **source_rev** | 3b0a3d7 |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | XDG Shell popup hierarchy and hit-testing |
| **affected_branches** | experimental/inc7-exit-readiness-2026-10-14 |
| **related_invariants** | W-SM-20, X-15, OB-1 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | Maintain test controls in verify-test-efficacy |

---

## Verified Facts
1. **The Upstream Defect (W-SM-20):** In stock Smithay 0.7.0, `PopupTree` enumerates a popup's children oldest-first, and root popups in creation order. Consequently, an older sibling popup (or an older popup's subtree) is rendered and hit-tested in front of a newer popup, violating XDG Shell protocol requirements.
2. **The Decision:** The adapter assigns a monotonic sequence number to each XDG popup at creation (`new_popup`). The snapshot generator stably sorts popup trees newest-first. A child is always newer than its parent, so it remains in front. Core concatenates the order supplied by the adapter.

## Unverified Hypotheses
- None; verified via pixel-exact rendering tests and pointer hit-test verification.

## Performance & Correctness Implications
- Guarantees protocol compliance: newest submenus and overlapping context menus stack cleanly above older menus.

## Limitations & Counterexamples
- Deviates from stock Smithay `Window::render_elements` order (deviation W-SM-20). Sealed text amendment X-15 prepared for approval.

## Reproduction Instructions
Execute the popup stacking order test:
```bash
cargo test -p infynitae-backend-headless --test t1_inc7_adversarial a_newer_popup_is_stacked_above_every_older_one
```

## Links & Citations
- Source Commit: [`3b0a3d7`](file:///home/vysaely/infynitae-native) (*"fix(adapter): popups of a toplevel are stacked newest in front, as xdg-shell requires (P10 #3)"*)
- Related Research: [KNOW-RES-003: DriftWM Popups and Lock Reentrancy](../research/KNOW-RES-003-driftwm-popups-and-locks.md)
