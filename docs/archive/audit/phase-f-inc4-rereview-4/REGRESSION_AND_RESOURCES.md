# Fifth Review — Ratified INC-3 Regression Surface & Resource Integrity

## 1. INC-3 Ratified Regression Surface

All guarantees established and ratified in Phase F INC-3 (`1ee520fa0079d45193f2a7274b31018e9276b0ed`) were re-verified independently:

1. **InputPort & Authorization Authority:**
   - Intake strictly enforces `InputPort` capability. Foreign ports rejected without state mutation.
   - Verified by `intake_rejects_foreign_ports_and_non_finite_values` and `set_focus_rejects_foreign_port`.
2. **CameraPan & INV-INPUT-3:**
   - Middle-button + Super acquires `CameraPan`.
   - `Focus::Keep`: Zero pointer motion, relative motion, or button events reach clients during a pan.
   - Exactly one true-position motion event emitted upon pan release.
   - Panning at persistent zoom $\ne 1$ forbidden (`no_pan_at_persistent_zoom`).
   - Verified by `x8_c1_keepfocus_pan_then_true_position` and `a_pan_emits_no_client_effect_and_ends_with_one_true_position`.
3. **Zero Outputs & Reconnect (M10):**
   - Disconnecting all outputs installs zero-output grab; frees pointer focus without fabricating historical focus upon reconnect.
   - Reconnecting output removes zero-output grab cleanly.
   - Verified by `zero_output_transition_free_pointer` and `zero_output_transition_with_a_held_button`.
4. **ProjectionFrame & Damage Tracking:**
   - Camera movement invalidates `ProjectionFrame`.
   - Transform and hit-test agreement holds across fractional scales.
   - Verified by `ee1_c5_render_hit_agreement_on_the_production_path`.

## 2. Resource Boundedness & Long-Lived Stress

### Shipped 200-Cycle Test (`round4_long_lived_combined_stress`)
- Ran 200 cycles of combined popup churn (depths 1..3), client-driven destruction, compositor dismissal, CameraPan with concurrent destruction, duplicate modifiers, zero outputs and reconnect, and $>64$ provenance events.
- Results:
  - File descriptors: 10 $\rightarrow$ 10 (0 leaked descriptors).
  - Protocol violations: 0.
  - Modifiers: 0 depressed.
  - Provenance store: strictly $\le 64$.
  - Grabs: none.

### Independent 250-Cycle Attack Test (`attack_250_cycle_deep_stress_session`)
- Ran 250 cycles of deep chains (depths 1..4), middle-popup destruction, CameraPan, focus transfers, duplicate modifiers, and reconnects.
- Results:
  - Protocol violations: 0.
  - Modifiers: 0 depressed.
  - Ownership tables: clean (0 pressed keys, 0 pressed buttons).
  - Smithay and Core focus: coherent.
  - Grabs: completely clear.
