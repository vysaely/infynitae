# Fifth Review — Obligation Review (Phase F INC-4)

This document details the disposition of canonical obligations evaluated during the Fifth Independent Review.

## OB-4: Interaction Acquisition Dismisses Popup Grabs
- **Statement:** Acquisition of a compositor interaction (e.g. `CameraPan`) must dismiss active popup grabs and return input focus to the compositor interaction without leaking pointer motion to clients.
- **Verification Status:** **SATISFIED**.
- **Evidence:** `PanDismissedPopupGrab` replaces `PopupPointerGrab` at pan start with `Focus::Keep`, guaranteeing 0 client events during the pan. At pan end, `restore_focus: false` ensures 0 synthetic duplicate motion events. Verified by `pf_ob4_interaction_acquisition_dismisses_popup_grab`, `r10_pan_with_popup_no_synthetic_events_and_clean_return`, and `a1_camerapan_overlapping_destruction`.

## OB-15: Keyboard Focus and Modifier State Truthfulness
- **Statement:** The compositor must truthfully track keyboard focus, modifier state, and key transitions without divergence between Core and the Wayland seat, and without stuck modifiers across any sequence.
- **Verification Status:** **SATISFIED**.
- **Evidence:** Centralized duplicate transition filter in `sync_seat_key`, capability-sealed `InputAuthToken`, strict modifier synchronization across all physical keys and contexts, and clean 0-depressed modifier end state across 250 stress cycles. Verified by `t04_duplicate_presses`, `t02b_token_is_bound_to_its_core`, and `r17_long_lived_stress_bounded_state`.

## OB-18: Popup Lifecycle
- **Statement:** The compositor must handle popup creation, positioning, grab acquisition, dismissal, and destruction in compliance with XDG Shell and Wayland core protocol rules.
- **Verification Status:** **SATISFIED (Closable at Formal Ratification)**.
- **Evidence:** `reconcile_popup_chain` and `Core::toplevel_tree_changed` ensure popup destruction cleans up grabs, restores keyboard and pointer routing, updates scene geometry, and never corrupts parent window lifecycle.

## OB-21: Focus Follows Mouse / Click-to-Focus
- **Statement:** Focus transitions following pointer events must be authorized and consistent.
- **Verification Status:** **OPEN for INC-16 / T4** (Click-to-focus baseline implemented and verified by `click_to_focus_focuses_mapped_window`).

## OB-30: Popup Focus, Preemption, and Grab Lifecycle
- **Statement:** Preemption of popup grabs by element focus changes must execute ordered teardown (`PopupGrab::ungrab` $\rightarrow$ `keyboard.unset_grab` $\rightarrow$ `pointer.unset_grab`) adhering to W-SM-15 and avoiding transient client re-entry.
- **Verification Status:** **SATISFIED**.
- **Evidence:** Verified by `ordered_preemption_exact_keyboard_wire` (asserting exact wire sequence `popup_done` $\rightarrow$ `wl_keyboard.leave` $\rightarrow$ `wl_keyboard.enter` $\rightarrow$ `wl_keyboard.modifiers`), `pf1_grab_active_core_focuses_w2_ordered`, and `t06a_three_level_nested_distinct_serials_focus_transfer`. NEG-1, NEG-2, OBS-1 permanently gated in `verify-architecture` [5d].
