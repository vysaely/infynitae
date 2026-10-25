# C12.F15 evidence — provenance

Produced 2026-10-17 in a scratch export of `270128f` (`git archive 270128f`; the C12.F14 tree, identical
to local `main` at the time), with its own cargo target directory. Nothing here is compiled by the
workspace; the repository's code is unchanged by C12.F15. The production wrapper is INC-10's work.

| File | What it is |
|---|---|
| `prototype_convert.rs.diff` | the option B prototype: `CanonicalRect<E>` in `infynitae-wayland/src/convert.rs` (geometry = Core's rect; `draw` passes `dst` through; damage full and opaque regions empty when remapped; crop to ±2³⁰ with `CropRenderElement`'s `src` formula); `render_output` passes the frame's k |
| `prototype_headless_lib.rs.diff` | `render_frame` returning the wrapped elements |
| `render_hit_sweep.rs` | sweep test: production `View::new` at fit-like zooms, k by single ulps around A's right-edge rounding point, real `render_elements` / `geometry`, real `ProjectionFrame::select`; adjacent subsurfaces A\|B; random-rate and exhaustive k = n/120 arithmetic checks |
| `render_hit_sweep_current_handoff.out` | the sweep on the unmodified sources (`270128f`) |
| `render_hit_sweep_prototype.out` | the sweep's real-Smithay part with the prototype (0 mismatches: geometry is Core's by construction; the pixel run is the decisive check) |
| `option_b_pixels.rs`, `option_b_pixels.out` | Pixman (software) pixels: current handoff versus wrapper at k = 0.7248438577698304; three frames through one damage tracker (full, unchanged, repaint at age 1); the crop's `src` arithmetic beyond i32 (its "blue columns in view" field is 0 because drawing was removed for those cases: Pixman cannot draw that magnification); the current handoff's saturated geometry for an edge at −2⁴⁰ (its height is an artefact of scaling both axes by the same k in that check) |
| `prototype_suite.out` | `cargo test --workspace --locked --no-fail-fast` on the prototype: 457 passed, 1 failed (TST-04's i32-location negative control), 1 ignored (the NB-1 runtime check) |

Run (in an export with the prototype applied): copy the two `.rs` files to
`crates/infynitae-backend-headless/tests/`, then
`cargo test --release -p infynitae-backend-headless --test render_hit_sweep -- --nocapture` and
`cargo test -p infynitae-backend-headless --test option_b_pixels -- --nocapture`.

Limits: software renderers only (DummyRenderer geometry, Pixman pixels) — no GLES, GPU, dmabuf,
direct scanout or hardware plane is exercised; Pixman returns `Unsupported` beyond a magnification of
about 2¹⁵; the crop was checked for buffers without a buffer transform only.
