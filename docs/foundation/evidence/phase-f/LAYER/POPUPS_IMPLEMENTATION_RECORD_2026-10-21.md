# Layer popups — implementation record (2026-10-21)

Status: **IMPLEMENTED on branch `feature/layer-popups-2026-10-21`** (from `main` 96d8ae5); not
pushed. It works inside the layer decisions L1–L8 (`DECISION_RECORD_2026-10-21.md`) and needs no
new decision. C12.F19 is still prepared, not adopted; acceptance is the owner's. This record follows
`IMPLEMENTATION_RECORD_2026-10-21.md` and closes the first item of its "Not done yet" list. That
record is not edited.

## What it does

A popup of a layer surface (`xdg_surface.get_popup` with no parent, then
`zwlr_layer_surface_v1.get_popup`) is what Quickshell and other panels use for menus and tooltips.

- **Tree:** the popup is part of its layer surface's tree. It is drawn with the surface, in front
  of it, newest popup first, as for windows. It is hit-tested before the Canvas for `top` and
  `overlay` surfaces (L2, L3), in its own surface-local frame. Core needed no new drawing or
  hit-test code, because F-ORIGIN-1 already places popups in a tree.
- **Placement:** the popup is unconstrained against its surface's whole output, in the layer
  surface's own coordinates. A layer surface never moves with a camera, so the output is where its
  popups can be seen. Edges are rounded inward at a fractional scale (R8). Core:
  `layer_popup_constraint_target`.
- **Scale:** the popup is told its surface's preferred scale before its initial configure. Core:
  `layer_preferred_scale`.
- **Grab (menus):** a grab is granted only while the user is using that panel, meaning the
  pointer is on its tree or it holds the keyboard (L5). Otherwise the menu is dismissed at once
  (`popup_done`). While the grab lasts, the menu holds the pointer and keyboard. A click outside it
  dismisses it, and that click belongs to whatever it lands on.
- **End of a popup chain:** the keyboard returns to the layer surface holding it (L5), if one
  does, and otherwise to the focused window. This also corrects the window-popup case, which
  ignored a layer holder.
- **Pointer (M10):** a layer tree that changes shape (a popup opens or closes, or a surface's
  size, place or input region changes) gives the pointer one true-position event, as a change of
  the surface itself does. A new frame request alone is not a change of shape.
- **Unmap:** a layer surface that unmaps dismisses its popups, as a window does.

## Evidence

The T0 tests are in `crates/infynitae-core/src/layer.rs` and the T1 tests in
`crates/infynitae-backend-headless/tests/t1_layer_shell.rs`, using the probe's new `layer_popup`
helper.

| Behaviour | Tests | Seeded control ("layer popups: …") |
|---|---|---|
| Popup in the layer tree, drawn and hit in front | T0 `a_bars_popup_is_drawn_and_hit_in_front_of_the_window_under_it`; T1 `a_bars_menu_holds_the_pointer_and_keyboard_and_gives_them_back` | snapshot leaves popups out |
| Placement against the whole output | T0 `a_layer_popup_is_constrained_to_the_whole_output`; T1 `a_bottom_bars_menu_is_slid_into_the_output` | not unconstrained; target coordinates; get_popup does not place |
| Grab only for a panel in use | T1 `a_menu_grab_from_a_panel_not_in_use_is_refused`, `a_click_outside_a_bars_menu_dismisses_it` | granted when not in use; refused when in use |
| Keyboard back to the layer holder | T1 `…gives_them_back` | keyboard returns to the window |
| Destroyed popup leaves the tree; pointer re-routed | T1 `…gives_them_back` | destroyed popup stays; no M10 on reshape |
| Unmapped surface dismisses its popups | T1 `a_bar_that_unmaps_dismisses_its_menu` | unmapped surface keeps its popups |

## Not done yet [F]

- A popup of a `bottom` or `background` surface is drawn with its surface, below the Canvas.
  Panels on those layers are rare; the popup could be lifted above the Canvas if one needs it.
- Pointer constraints on layer surfaces, presentation feedback for layer surfaces, and a live
  Quickshell run are as listed in the earlier record.
