# Layer surfaces — implementation record (2026-10-21)

Status: **IMPLEMENTED on branch `feature/layer-shell-2026-10-21`** (from `work/polish-2026-10-21`);
not `main`, not pushed; C12.F19 prepared, not adopted; acceptance is the owner's.

## Evidence

| Decision | Tests (T0 `crates/infynitae-core/src/layer.rs`, T1 `crates/infynitae-backend-headless/tests/t1_layer_shell.rs`) | Seeded control |
|---|---|---|
| L1 arrangement, configure | `exclusive_zones_arrange_in_order_and_shrink_the_usable_area`, `a_buffer_before_the_initial_configure_is_refused`; T1 `a_bar_reserves_its_strip_…` | L4 zone |
| L2 draw / frame callbacks | T1 `a_bar_reserves_its_strip_…` (the bar's callback completes) | L2 no frame callbacks |
| L3 hit order, click | `hit_testing_follows_the_layer_order_…`; T1 `the_pointer_and_a_click_reach_the_bar_…` (precondition: the window lies under the point) | L3 ×2 |
| L4 placement, fit | `placement_and_fit_use_the_usable_area`; T1 placement below the bar | L4 placement |
| L5 keyboard | `keyboard_interactivity_follows_the_protocol`; T1 `an_exclusive_launcher_takes_the_keyboard_and_gives_it_back` | L5 ×2 |
| L6 presentation | `a_presentation_hides_all_but_overlay_surfaces` | L6 |
| INV-OUTPUT-1 | `an_output_removal_closes_its_layer_surfaces_first`; T1 `removing_the_output_closes_…` | output removal |

## Not done yet (the next steps) [F]

- **Popups of layer surfaces** (`zwlr_layer_surface_v1.get_popup`), which Quickshell's menus and
  tooltips need, aren't implemented: such a popup gets no parent placement.
- **Pointer constraints on layer surfaces** (L7, second half) aren't implemented: constraints
  follow element surfaces only. A pan that starts on a layer surface pans (L7, first half).
- **Presentation feedback** isn't sent for layer surfaces; their frame callbacks are.
- **A hidden layer surface** (`top`, `bottom` or `background` while the output presents) gets no
  frame callbacks until it is shown again.
- **Live check:** Quickshell hasn't been run against this yet; that needs a nested run.
