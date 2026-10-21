# Super+Space recenter — implementation record (2026-10-21)

Status: **IMPLEMENTED on branch `feature/recenter-2026-10-21`** (from `main` 61e344b); not pushed.
Source: the owner's interaction intent of 2026-10-05 ("Super+Space recenter camera on focused
element"), which the owner restated on 2026-10-21: "would actually center it", not fit it. See
`KEYBINDINGS_INTENT_2026-10-21.md`, which is not edited.

## Behaviour

- **Key:** Super+Space with exactly Logo held, never during an interaction. Super+Shift+Space
  stays unbound.
- **Target:** the camera of the output holding the pointer is centred on the focused window.
  The centre of its window geometry goes to the centre of the output's usable area (L4), so a
  bar does not push the window off-centre.
- **Zoom and history:** the zoom stays as it is. No history entry is made, as for a shell
  `Navigate` (OBJ-30 records fits), so Super+Enter's restore is unaffected.
- **Refusals** (nothing changes): no output holds the pointer; that output is presenting a
  fullscreen window (C12.F17 Q1 B); nothing is focused; the focused window is not mapped or has
  a degenerate geometry.
- **After the move:** the same consequences as a fit — frame invalidated, membership and scale
  pass, and one true-position pointer event (M10).
- The proof is minted in `physical_input` with the fit bindings (INV-CAMERA-1).

## Evidence

| Behaviour | Tests | Seeded control ("recenter: …") |
|---|---|---|
| Key resolution | `keys::tests::logo_space_recenters_with_exact_modifiers`; T1 `t1_recenter` (Shift not bound) | resolves nothing |
| Centre, zoom kept, no history, M10 | T1 `super_space_centres_the_focused_window_and_keeps_the_zoom` | no proof; camera not written; pointer not reconciled |
| No focus: nothing | T1 `super_space_without_a_focused_window_or_with_shift_changes_nothing` | recenters on some window |
| Usable-area centre | `navigation::tests::recenter_puts_the_window_centre_at_the_usable_centre` | usable area ignored |
