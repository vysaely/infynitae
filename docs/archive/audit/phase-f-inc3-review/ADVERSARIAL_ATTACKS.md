# Phase F Increment 3 (INC-3) — Adversarial Attacks & Mutation Efficacy Audit

---

## 1. Twenty Independent Adversarial Scratch Mutations (A through T)

Twenty independent adversarial mutations were evaluated against the implementation in an isolated scratch worktree. Every single mutation was successfully detected by the existing test suite:

| ID | Title | Injected Mutation | Target Test | Detection Verdict | Failing Assertion / Diagnostic |
|---|---|---|---|---|---|
| **A** | Camera pan sign inversion | `c.saturating_add(q16(s))` in `navigation.rs` | `t1_input::x8_c1_keepfocus_pan_then_true_position` | **DETECTED** | `assertion failed: (r.since(m1) == ["motion 11,283", "frame"])` |
| **B** | Pan writes Stage anchor | Mutate element `anchor_x` in `entry.rs` during pan | `t1_adversarial::m_pan_never_writes_the_stage` | **DETECTED** | `assertion failed: (r.anchor() == anchor)` |
| **C** | KeepFocus replaced by rerouting | Forward `routed_motion` during pan in `entry.rs` | `t1_input::x8_c1_keepfocus_pan_then_true_position` | **DETECTED** | `assertion failed: !l.starts_with("motion")` |
| **D** | Pan produces relative motion | Synthesize `PointerRelative` at reconciliation | `t1_input::x8_c1_keepfocus_pan_then_true_position` | **DETECTED** | `assertion failed: (r.since(m1) == ["motion 11,283", "frame"])` |
| **E** | Compositor MMB leaks to client | Emit `PointerButton` on pan acquisition | `t1_input::x8_c1_keepfocus_pan_then_true_position` | **DETECTED** | `assertion failed: !l.starts_with("button")` |
| **F** | Client button swallowed | Ignore client ownership when pan fails | `t1_input::x8_c6_no_acquisition_while_the_client_owns_a_button` | **DETECTED** | `assertion failed: (r.since(m0) == ["button 272 pressed", "frame"])` |
| **G** | Output removal does not orphan pan | Omit `orphan_if_on(output)` in `entry.rs` | `t1_input::removing_the_pan_output_orphans_the_pan` | **DETECTED** | `assertion failed: (r.ls.state.core().interaction() == Interaction::Orphaned)` |
| **H** | Orphaned pan moves survivor | Forward motion during `Orphaned` | `t1_input::removing_the_pan_output_orphans_the_pan` | **DETECTED** | `assertion failed: (r.camera() == c0)` |
| **I** | Zero-output grab fails to clear focus | Use `Focus::Keep` instead of `Focus::Clear` | `t1_input::zero_output_transition_free_pointer` | **DETECTED** | `assertion failed: (r.since(m0) == ["leave W"])` |
| **J** | NoOutput forwards button | Forward client button during NoOutput | `t1_input::zero_output_transition_with_a_held_button` | **DETECTED** | `assertion failed: (r.since(m0).is_empty())` |
| **K** | Reconnect restores stale focus | Omit `pending_focus = None` in adapter | `t1_input::zero_output_transition_free_pointer` | **DETECTED** | `assertion failed: (r.since(m1) == ["enter W ..."])` |
| **L** | Boundary tie picks last output | Change `<` to `<=` in `pointer.rs` | `infynitae_core::locate_clamps_to_the_union_of_output_rects` | **DETECTED** | `assertion failed: (located == (id0, ...))` |
| **M** | Output crossing uses source camera | Use source frame in `hit_test` | `t1_adversarial::e_output_transition_uses_the_new_outputs_camera` | **DETECTED** | `assertion failed: (positional(&r, m) == ["enter W 61,20"])` |
| **N** | Pan retains stale ProjectionFrame | Retain frame on pan in `entry.rs` | `infynitae_core::a_pan_drops_the_output_frame` | **DETECTED** | `assertion failed: (self.frames.is_empty())` |
| **O** | Hit test uses unmapped surface | Omit `entry.shown` check in `render.rs` | `t1_adversarial::k_unmapped_surface_receives_nothing` | **DETECTED** | `assertion failed: (positional(&r, m).is_empty())` |
| **P** | Input region hole ignored | Bypass input region check in `render.rs` | `t1_adversarial::j_input_region_holes_fall_through` | **DETECTED** | `assertion failed: (positional(&r, m) == ["enter W ..."])` |
| **Q** | Physical pixel floor changed to round | Use `.round()` in `projection.rs` | `t1_adversarial::a_physical_pixel_boundary` | **DETECTED** | `assertion failed: (r.ls.probe.ptr == ...)` |
| **R** | Arrangement coords substituted | Deliver arrangement coords in `route_pointer` | `t1_input::x8_c1_keepfocus_pan_then_true_position` | **DETECTED** | `assertion failed: (r.since(m1) == ["motion 11,283", "frame"])` |
| **S** | Saturating bound removed/wrong | Remove `.clamp(-limit, limit)` in `navigation.rs` | `t1_adversarial::d_large_canvas_coordinates_near_the_bound` | **DETECTED** | `assertion failed: (r.camera().x == (1i64 << 36) << 16)` |
| **T** | Final reconciliation omitted | Return `Vec::new()` at pan release in `entry.rs` | `t1_input::x8_c1_keepfocus_pan_then_true_position` | **DETECTED** | `assertion failed: (r.since(m1) == ["motion 11,283", "frame"])` |

---

## 2. Discovery of Level B Finding `INC3-F09` (File Descriptor Leak)

While executing test suites under standard Linux environment conditions (`ulimit -n 1024`), the reviewer uncovered an environment portability bug:
- `t1_transform::ee1_c5_render_hit_agreement_on_the_production_path` panics with:
  `shm file: Os { code: 24, kind: TooManyOpenFiles, message: "Too many open files" }`
- Traced to `tile_view` allocating 192 subsurfaces each holding a temporary unlinked shm file descriptor, compounded across 10 iterations of `Lockstep::new` without explicit socket client reaping or single-pool sharing.
- Under `ulimit -n 4096`, all tests pass cleanly.
- Classified as **Level B** finding `INC3-F09`.
