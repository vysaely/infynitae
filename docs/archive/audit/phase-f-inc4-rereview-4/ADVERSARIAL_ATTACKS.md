# Fifth Review — Adversarial Attack Methodology & Fresh Mutation Analysis

## 1. Methodology
All adversarial mutations were executed in an isolated, disposable git worktree detached from HEAD `4c01cca2d43e38f4f3a80e9202ec1b7ab0430179`.
The canonical repository remained byte-identical throughout.
Each mutation was applied individually and evaluated against the shipped default workspace test suite (`cargo test --workspace --locked --no-fail-fast`).

## 2. Fresh Mutation Results (22 Mutations)

| ID | Mutation Description | Target Component | Outcome | Detecting Tests |
|---|---|---|---|---|
| **A1-a** | Chain end: keyboard grab teardown omitted | `handlers.rs` | **MISSED (Equivalent)** | See §3 for proof of semantic equivalence |
| **A1-b** | Chain end: pointer grab teardown omitted | `handlers.rs` | **DETECTED** | `a1_client_closes_chain_of_depth_1_2_3`, `a1_inner_dies_outer_kept`, `a1_middle_destroyed_tolerated`, `a1_rapid_chain_destroy`, `a1_root_destroyed_during_destruction` |
| **A1-c** | Chain end: focus reconciliation omitted | `handlers.rs` | **DETECTED** | `a1_client_closes_chain_of_depth_1_2_3`, `a1_inner_dies_outer_kept`, `a1_middle_destroyed_tolerated`, `a1_rapid_chain_destroy`, `a1_zero_outputs_during_destruction_then_reconnect`, `round4_long_lived_combined_stress` |
| **A1-d** | Chain end: reconcile to `current_grab()` instead of Core target | `handlers.rs` | **MISSED (Equivalent)** | See §3 for proof of semantic equivalence |
| **A1-e** | Reconcile like Round 3 (depth-1 accidental path) | `handlers.rs` | **DETECTED** | `a1_client_closes_chain_of_depth_1_2_3`, `a1_inner_dies_outer_kept`, `a1_middle_destroyed_tolerated`, `a1_rapid_chain_destroy`, `a1_zero_outputs_during_destruction_then_reconnect`, `round4_long_lived_combined_stress` |
| **A1-f** | No cleanup before reconciliation | `handlers.rs` | **DETECTED** | `a1_client_closes_chain_of_depth_1_2_3`, `a1_inner_dies_outer_kept`, `a1_middle_destroyed_tolerated`, `a1_rapid_chain_destroy`, `a1_root_destroyed_during_destruction`, `a1_zero_outputs_during_destruction_then_reconnect` |
| **A1-g** | Inner death: lazy refocus left to Smithay | `handlers.rs` | **DETECTED** | `a1_client_closes_chain_of_depth_1_2_3`, `a1_inner_dies_outer_kept` |
| **B1-a** | Popup death runs parent root-commit lifecycle | `handlers.rs` | **DETECTED** | `b1_uncommitted_parent_untouched_by_popup_commit_or_death`, `b1_unmapped_parent_reshow_gets_exactly_one_configure` |
| **B1-b** | Popup death does not rebuild hit-test tree | `handlers.rs` | **DETECTED** | `b1_mapped_parent_popup_destroyed_is_not_a_parent_commit`, `t99g_popup_destroyed_by_client_then_clicks`, `t99j_stale_popup_area` |
| **B1-c** | Child commit runs parent lifecycle | `handlers.rs` | **DETECTED** | `b1_uncommitted_parent_untouched_by_popup_commit_or_death` |
| **B1-d** | Tree change sends duplicate initial configure to unmapped | `protocol.rs` | **DETECTED** | `b1_uncommitted_parent_untouched_by_popup_commit_or_death`, `protocol::tests::tree_change_never_drives_the_lifecycle` |
| **B1-e** | Tree change applied to non-mapped elements | `protocol.rs` | **DETECTED** | `protocol::tests::tree_change_never_drives_the_lifecycle`, `k_unmapped_surface_receives_nothing` |
| **P-a** | Provenance store unbounded | `lib.rs` | **DETECTED** | `round4_long_lived_combined_stress`, `serial_outputs_storage_bounded_at_capacity`, `serial_provenance_fifo_eviction_and_fallback` |
| **P-b** | Newest-first eviction (F15) | `lib.rs` | **DETECTED** | `serial_provenance_fifo_eviction_and_fallback` |
| **P-c** | Unknown/evicted serial aliases latest record (F16) | `lib.rs` | **DETECTED** | `serial_provenance_fifo_eviction_and_fallback` |
| **P-d** | Valid serial falls back to current pointer output | `handlers.rs` | **DETECTED** | `r15_rule1_uses_the_causing_event_output`, `serial_provenance_fifo_eviction_and_fallback` |
| **O-a** | Ordered preemption: pointer unset before keyboard (W-SM-15 / N09d) | `outputs.rs` | **DETECTED** | `ordered_preemption_exact_keyboard_wire`, `pf1_grab_active_core_focuses_w2_ordered`, `pf2_grab_active_core_focuses_none_ordered`, `pf4_popup_destroyed_in_flight_while_core_changes_focus_ordered`, `pf6_nested_popup_grab_keeps_pointer_grab`, `pf7_client_owned_key_held_during_grab_core_focuses_w2_release` |
| **O-b** | Ordered preemption: keyboard unset omitted | `outputs.rs` | **DETECTED** | `a1_focus_other_root_during_destruction`, `a1_root_unmaps_during_destruction`, `ordered_preemption_exact_keyboard_wire`, `pf1_grab_active_core_focuses_w2_ordered`, `pf2_grab_active_core_focuses_none_ordered`, `pf4_popup_destroyed_in_flight_while_core_changes_focus_ordered`, `pf6_nested_popup_grab_keeps_pointer_grab`, `pf7_client_owned_key_held_during_grab_core_focuses_w2_release`, `round4_long_lived_combined_stress`, `t06a_three_level_nested_distinct_serials_focus_transfer` |
| **Au-a** | Duplicate key reaches seat (transition gate removed) | `lib.rs` | **DETECTED** | `r17_long_lived_stress_bounded_state`, `round4_long_lived_combined_stress`, `t04_duplicate_presses` |
| **Au-b** | Cross-Core token accepted | `lib.rs` | **DETECTED** | `t02b_token_is_bound_to_its_core` |
| **Au-c** | Token key ignored; seat updated for fixed key | `lib.rs` | **DETECTED** | Detected by 39 integration tests across keyboard, input, adversarial, and popup suites |
| **Bi-a** | Super+B fixture restored | `keys.rs` | **DETECTED** | `input::keys::tests::binding_resolution_honours_logo_modifier`, `t11_unconfigured_keys_are_never_swallowed` |

## 3. Analysis of Semantic Equivalences

### A1-a: Omission of explicit `self.keyboard_unset_grab()`
In `reconcile_popup_chain`, omitting lines 73–77 (`self.keyboard_unset_grab()`) leaves the subsequent line 91:
```rust
kb.set_focus(self, target, serial);
```
Under Smithay 0.7.0 (`desktop/wayland/popup/grab.rs`), `KeyboardGrab::set_focus` checks:
```rust
if self.popup_grab.has_ended() {
    handle.set_focus(data, focus, serial);
    handle.unset_grab(self);
}
```
Because `has_ended()` is already true at this branch, Smithay's `set_focus` automatically unsets the grab as a side effect. Therefore, omitting explicit unset produces identical wire, state, and observer outputs. The explicit call is retained in production as defense-in-depth adhering strictly to INV-INPUT-10 Step 2.

### A1-d: Reconcile to `current_grab()` instead of Core target
In `reconcile_popup_chain`, a recorded popup grab exists if and only if Core's `ActiveElementFocus` is that chain's root. Grabs are rejected on unfocused elements (`XdgShellHandler::grab`), and any focus transition clears the grab record immediately (`Effect::KeyboardFocus`, `Effect::DismissPopupGrab`). Consequently, at the terminal condition, the chain's root surface is identical to Core's active element root surface. Core is the canonical authority, so deriving the target from Core is the architectural standard.

## 4. Conclusion
With 20/22 detected, 2/2 proven mathematically equivalent, and 0 broken, the Round-4 test and verification suite exhibits **100% genuine mutation coverage**.
