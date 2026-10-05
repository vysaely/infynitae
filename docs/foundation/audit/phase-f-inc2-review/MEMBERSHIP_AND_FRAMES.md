# Membership, Damage Trackers, Frame Clock & Presentation Audit

## 1. Surface Membership (`F-MEMBER-1`, `INV-RENDER-3`)

- **Formula `F-MEMBER-1`:** Output membership is computed per surface in `view_O` Canvas space. A surface is on output $O$ if and only if some physical pixel of it projects onto $O$'s viewport.
- **Implementation:** In `crates/infynitae-core/src/visibility.rs`, `MembershipTable::compute()` iterates over `stage.front_to_back()` and `views`. For each output view, it calls `render::entries_for()`.
- **Enter/Leave Diffs:** `MembershipTable::diff()` emits `Effect::Leave` first, then `Effect::Enter`.
- **Dead Client Handling:** If a client was destroyed, `stage.state(e)` is `None`, so `diff()` suppresses emitting `Leave` to non-existent Wayland resources.
- **X5 Popup Membership Control:** Verified in `t1_outputs.rs::x5_popup_membership_and_output_enter_control`. When a popup is positioned outside an output's view, Infynitae correctly emits zero `enter` events. The negative control proves that Smithay's stock `Window::output_enter` (as driven by `Space`) incorrectly enters the invisible popup.

---

## 2. Damage Trackers & EE-1 Transform Matrix

- **Dimensions:** Sized to `phys_O` (untransformed physical mode `(mode.0, mode.1)`), not `view_O`:
  ```rust
  pub fn damage_tracker(model: &OutputModel, view: &View) -> OutputDamageTracker {
      OutputDamageTracker::new(
          (model.phys.0 as i32, model.phys.1 as i32),
          Scale::from(view.k),
          output_transform(model.transform),
      )
  }
  ```
- **EE-1 Verification:** `crates/infynitae-backend-headless/tests/t1_transform.rs` executes the full 10-case EE-1 matrix across Normal, R90, R180, R270, 90°/270° with fractional scales (1.25, 1.5), and Flipped variants.
- **Checks C1–C4:**
  - C1: Render viewport equals physical panel.
  - C2: View covers physical panel exactly (zero blank, zero lost pixels).
  - C3: 4 edge markers land on panel boundaries at full length.
  - C4: Full-frame damage equals view extent.
- **Negative Control:** Sizing the tracker to `view_O` fails all 7 rotated cases (and 0 unrotated cases).

---

## 3. Frame Clock Selection (`INV-FRAME-2`)

- **Rule:** The frame clock of an element is its highest-refresh observing output. Ties are broken by lowest `OutputId`.
- **Implementation:** `visibility::clock_output(observers, outputs)`.
- **Sliver Independence:** In `clock_does_not_depend_on_visible_area`, an output observing only a 20 px sliver of an element at 165 Hz correctly clocks the element over an output observing the entire element at 144 Hz.
- **Starve, Re-Entry, and Handover:** Verified in `starve_reentry_and_handover`:
  - When all observing outputs are disconnected, frame callbacks are withheld (0 completions).
  - On re-entry, the first callback completes at the exact first vblank of the new clock output ($t_{\text{on}} + 6{,}060{,}606\text{ ns}$).
  - On handover (clock output unplugged while another observer survives), pending callbacks complete at the next vblank of the surviving clock.

---

## 4. Presentation Feedback (`INV-FRAME-5`)

- **Observed Scope:** Implemented in `outputs.rs::complete_frames()` via `OutputPresentationFeedback`.
- **Lifecycle:** Verified in `t1_outputs.rs::x9_presentation_feedback`:
  - Commit #1 presented with `sync_output` matching clock output.
  - Commits #2 and #3 committed while unobserved: commit #2 discarded on supersession by #3.
  - Commit #3 held pending while unobserved; presented upon re-entry.
- **Future Increments:** Unobserved kick and throttle fallback timers are canonically owned by INC-8.
