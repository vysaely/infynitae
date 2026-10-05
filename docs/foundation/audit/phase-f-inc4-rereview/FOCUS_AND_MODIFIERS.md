# INC-4 Re-Review — Focus (OBJ-22/M11), Modifiers, Activation

## 1. Modifier authority — **FAIL (F16 partially resolved)**
- Keys: `sync_seat_key` runs Smithay `input_intercept` (XKB update) and returns the post-update `modifier_state()`; Core decides on that. Same-boundary semantics hold (R05: Super→MMB pans, MMB→Super does not, Super released immediately before MMB does not; no one-event lag). N01 (pre-update snapshot) survives but is equivalent for the shipped bindings.
- Buttons: `pointer_button` sends `snapshot.logo || seat.logo` (and the other mods). The backend snapshot is therefore still a modifier authority: `logo` in the snapshot with no Super held acquires a CameraPan (R04). Mutation N03 (seat-only, the canonical OBJ-27 design) breaks 15 shipped tests — the suite itself depends on the second authority.
- Zero-output / reconnect while a modifier is held: seat XKB is output-independent; R17 (150 cycles including Super held across pan and outputs) leaves no residual key/button ownership.

## 2. ActiveElementFocus / M11 — **FAIL** (one Level-A divergence)
- R06 sequence A→B→C, destroy C → B, unmap B → A, destroy B → A, destroy A → **Unfocused**, create D → still Unfocused: PASS. Arbitrary z-order fallback is gone (N15 detected).
- R07: `set_focus` on an unmapped element → `Err` (PASS; but N18 undetected by the shipped suite).
- Stage vs history liveness: history purges the lost element on every target-loss call (destroy and unmap), successor filtered by `mapped_elements()`; no divergence found.
- Popup as focus target: impossible through Core (targets are `ElementId`s; mapped check).
- **INC4R-01:** after OB-4 dismissal Smithay keyboard focus stays on the (then destroyed) popup while Core holds W1 (R16) — keys reach the client with no entered surface. Latent since `d5b7b95`.

**Focus fallback verdict:** PASS (F11 resolved; matches C4.5 literally).

## 3. Activation / INV-PROTO-1 — **PASS (behaviour and verification)**
`activated_tracks_active_element_focus` (port of ATK-08) uses the client-wire xdg_toplevel states; mutations Q (never unset) and R (never set) are both detected. Unfocused clears; destroy hands activation to the MRU successor; unmapped elements cannot be focused, hence never activated; popups never receive toplevel activation. Ordering observation from the first review stands (activation configures precede the preemption in a batch).
