# KNOW-DEC-004: Popup Stacking Order and Smithay Traversal Defect (W-SM-20)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-DEC-004 |
| **title** | Popup Stacking Order and Smithay Traversal Defect (W-SM-20) |
| **subsystem** | Wayland/Popups |
| **source_rev** | 3b0a3d7 (source line) → 23860b3, d350ed2 on main; layer C12.F8 a9ec294; ratified 39ad614 |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | XDG Shell popup hierarchy and hit-testing |
| **affected_branches** | main (f323c2d); historical: experimental/inc7-exit-readiness-2026-10-14 |
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
- Deviates from stock Smithay `Window::render_elements` order (deviation W-SM-20). ~~UNRATIFIED: the X-15 amendment (layer C12.F8, replacing C3.3 item 1) is prepared for owner approval, not adopted.~~ **Reconciliation 2026-10-14 (main `f323c2d`):** C12.F8 (INV-RENDER-2: a toplevel's `xdg_popup`s newest first) adopted (`a9ec294`) and **ratified with INC-7** (`39ad614`); OB-1 CLOSED. Dependence on the adapter's creation order is accepted residual R-08. Ported to `integration/inc-7-exit-2026-10-14` as `23860b3`. Related Smithay limitations: [KNOW-PITFALL-007](../pitfalls/KNOW-PITFALL-007-smithay-0-7-xdg-shell-limitations.md).

## Reproduction Instructions
Execute the popup stacking order test:
```bash
cargo test -p infynitae-backend-headless --test t1_inc7_adversarial a_newer_popup_is_stacked_above_every_older_one
```

## Links & Citations
- Source Commit: [`3b0a3d7`](file:///home/<user>/infynitae-native) (*"fix(adapter): popups of a toplevel are stacked newest in front, as xdg-shell requires (P10 #3)"*)
- Related Research: [KNOW-RES-003: DriftWM Popups and Lock Reentrancy](../research/KNOW-RES-003-driftwm-popups-and-locks.md)
