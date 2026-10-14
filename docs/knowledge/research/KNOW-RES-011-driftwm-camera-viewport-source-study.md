# KNOW-RES-011: DriftWM Camera, Viewport, Culling and Hit-Testing — Source Study at 352333a

| Field | Value |
| :--- | :--- |
| **id** | KNOW-RES-011 |
| **title** | DriftWM Camera, Viewport, Culling and Hit-Testing — Source Study at 352333a |
| **subsystem** | Research/DriftWM, Core/Navigation, Core/Input, Core/Render |
| **source_rev** | DriftWM 352333a8fa1b22171492d4b71a54102045c9a19d (Smithay git rev 4cf0b62028039661477d482ec4758b687d8f4392); Infynitae main c6d47c99c5ae20b84ab4ac7f2f3ad52a13f8d350 |
| **date** | 2026-10-15 |
| **evidence_class** | SOURCE-DERIVED |
| **applicability** | INC-6 (scale), INC-8 (frame callbacks), INC-14 (grabs, interactive move), any camera-animation, zoom or spatial-index proposal |
| **affected_branches** | main (c6d47c9) for every Infynitae statement |
| **related_invariants** | INV-CAMERA-1..5, INV-COORD-1..6, INV-RENDER-2, INV-FRAME-1, F-VIEW-1, F-LATTICE-1, F-PAN-1, F-HIT-1, F-MEMBER-1, F-PLACE-1 |
| **adoption_status** | RESEARCH |
| **next_action** | None required. Before quoting a DriftWM cost as a number, run the benchmark proposed under Unverified Hypotheses (needs owner approval) |

---

Executes the eight-question plan of KNOW-RES-009 (gap G-01). Owner approval: 2026-10-15.

**GPL boundary.** DriftWM is GPL-3.0-or-later. This record states concepts and `path:line` references only; it contains no DriftWM code and no paraphrase of it. Line numbers are at `352333a` in the local clone `~/.gemini/antigravity/scratch/driftwm` (unmodified, clean tree).

**Notation.** `W` mapped windows on the canvas (all outputs), `V` windows whose box meets an output's viewport, `O` outputs, `P` screen-pinned windows (DriftWM only), `S_V` surfaces of the visible windows, `R` membership rows (Infynitae).

**Evidence labels.** *Traced* = read in source along the named call path, not executed. *Inference* = a conclusion drawn from traced facts that the source does not state. Nothing here was run or measured.

## Verified Facts

### DriftWM (source-derived, traced at 352333a)

1. **Camera state.** *Traced.*
   - Each output carries an `OutputState` in its user data behind a `Mutex` (`src/state/mod.rs:428–430`, accessor `output_state` `:547`, which panics on re-entrant locking).
   - The camera is the **top-left** of the viewport in canvas logical px, an `f64` `Point` (`camera`), plus an `f64` `zoom`, with optional targets for animation (`camera_target`, `zoom_target`, `zoom_animation_anchor`), momentum and edge-pan state.
   - Zoom is clamped to `[MIN_ZOOM_FLOOR = 0.001, MAX_ZOOM = 1.0]` (`src/canvas.rs:10–12`). DriftWM never magnifies.
   - The centre is derived, not stored: `viewport_center` / `camera_for_center` (`src/canvas.rs:258`, `:272`), which also flip Y for the user-facing convention.
2. **World↔screen transform.** *Traced.*
   - The type split is `ScreenPos` / `CanvasPos` newtypes over Smithay `Point<f64, Logical>` (`src/canvas.rs:16`, `:20`).
   - `screen = (canvas − camera)·zoom` and its inverse (`screen_to_canvas` `:24`, `canvas_to_screen` `:33`), in `f64` with no quantisation.
   - Rendering applies `(loc − camera)·output_scale·zoom` per element at draw time (`src/render/mod.rs:875–884` for stand-ins; client windows go through the same camera/zoom pair).
   - Integer rounding happens at several independent points: `camera.to_i32_round()` for the Smithay output position and the cull rect (`src/state/output.rs:499–510`, `src/canvas.rs:401`), and a truncating `as i32` for the new-window seed (`src/handlers/xdg_shell.rs:45–54`).
3. **Viewport definition and updates.** *Traced.*
   - **Cull rect:** `visible_canvas_rect` is `camera_i32` plus `ceil(viewport/zoom) + 2` px per axis (`src/canvas.rs:401–409`). It depends on zoom.
   - **Smithay output position:** `update_output_from_camera` (`src/state/output.rs:499`) maps every output in Smithay's `Space` at `camera_i32`, O(O). When the integer camera changed, it calls `sync_pinned_locs`. `Space` holds no windows in DriftWM, only outputs (`dev/docs/caveats.md:7`).
   - **Output geometry ignores zoom:** Smithay's `Space::output_geometry` at `4cf0b62` (`src/desktop/space/mod.rs:384–400`) returns that location plus the **mode size / output scale**, so its size does not depend on DriftWM's zoom.
4. **Pan and zoom.** *Traced.*
   - **Pointer-drag pan** (`src/grabs/pan_grab.rs:60–108`): the screen delta is turned into `−delta·mouse_speed/zoom` and passed to `drift_pan_on` (`src/state/viewport_animation.rs:326`).
   - **`drift_pan_on`:**
     - It refuses on a fullscreen output.
     - It clears the targets and feeds momentum.
     - It adds the delta to the `f64` camera **per event**, so it accumulates and does not re-derive from the gesture origin.
     - It calls `update_output_from_camera` and re-arms one momentum timer per burst.
   - **Pointer during a pan:** the pointer location is kept in **canvas** coordinates. To hold the cursor still on screen, the grab moves the pointer's canvas location by the camera delta (`pan_grab.rs:99–107`). Animation paths do the same through `warp_pointer` (`viewport_animation.rs:181`).
   - **Momentum** (`apply_scroll_momentum` `:224`) and **zoom animation** (`apply_zoom_animation` `:466`) step per frame through `set_camera` / `set_zoom`.
   - **Zoom anchoring:** zoom keeps a canvas point fixed at a screen point (`zoom_anchor_camera`, `src/canvas.rs:472`) and warps the pointer to compensate (`viewport_animation.rs:523–531`).
   - **Fullscreen lock:** `set_camera_on` / `set_zoom` refuse while an output is fullscreen (`src/state/viewport.rs:45–68`). `drift_pan_on` and `enter_fullscreen` write `output_state` directly and carry their own guards.
5. **Visibility and culling (per rendered frame, per output).** *Traced.*
   - **The loop:** `compose_frame` (`src/render/mod.rs:718`) walks **every** stage element back to front (`:846`) and tests its box against `visible_canvas_rect` (`:866`, `:1014`).
   - **Linear lookups inside it:** for each client window it calls `Stage::position_of` (`:924`) and `Stage::id_of` (`:937`). Both resolve through `Stage::entry` (`src/stage/mod.rs:146–151`), a linear `find` over the z-order `Vec` (`:119`). The code itself notes that `id_of` is a linear scan (`src/render/mod.rs:935–937`).
   - **`post_render`** (`src/render/lifecycle.rs:249`) runs after each render. It walks every client window again with a `position_of` each (`:261–276`) to choose between a frame callback and the off-screen throttle (`FRAME_CALLBACK_THROTTLE`). It then calls `retain_alive` (O(W)) and `refresh_window_outputs` (`:352–353`).
6. **Pointer hit-testing.** *Traced.*
   - **When it runs:** `refresh_pointer_focus` (`src/input/mod.rs:950`) runs **once per event-loop iteration** (`refresh_and_flush_clients`, `src/state/mod.rs:1276–1295`) and once per rendered frame. It reads the pointer's canvas location and runs `focus_cascade` (`src/input/mod.rs:537`).
   - **The cascade:** screen-space layers, then pinned windows, then stand-ins, then non-widget canvas windows (`surface_under` `:1712`), then decorations, then canvas layers, then widget windows, then the bottom layers.
   - **The walks:** each canvas walk (`surface_under`, `element_under_skipping` `:1504`, `topmost_under` `:1576`) iterates `Stage::entries()` front to back. That iterator carries each position, so there is no per-entry lookup (`src/stage/mod.rs:241–244`). Each walk stops at the first hit.
   - **Scope:** the walks cover **all** stage entries, not a viewport-culled list. Per-entry checks are cheap rejections: pinned, deferred-adopt, and fullscreen-on-another-output (`find` over the fullscreen map, at most O(O)).
7. **Initial window placement.** *Traced.*
   - **Seed:** `new_toplevel` maps the window at once at a seed position, the camera centre of the active output (`src/handlers/xdg_shell.rs:38–54`, `map_window` `:70`), before any buffer. It marks the window `pending_center` (`:83`), and real placement happens at the first sized commit.
   - **Policy:** `window_placement` is `center` (default), `cursor` or `auto` (`src/config/mod.rs:508–511`).
   - **`auto`:**
     - It anchors to the focus snapshot taken at `new_toplevel`, or failing that to the nearest sufficiently visible element (`src/state/placement.rs:77`, `:113`).
     - It then calls `place_auto` (`src/layout/auto_placement.rs:79`).
     - `place_auto` grows a breadth-first cluster in which each visited window scans all windows for adjacency (`bfs_cluster` `:294–321`), then tries edge slots against the obstacle list.
8. **Work per window when the camera changes.** *Traced.*
   - **On the camera write:** O(1) to the value, plus `update_output_from_camera` O(O).
   - **Pinned windows:** if any exist and the integer camera changed, `sync_pinned_locs` (`src/state/pinned.rs:157–182`) **rewrites the canvas position of every pinned window**, each through a linear `set_position`. This is the one place a camera move changes window geometry.
   - **Everything else is not triggered by the camera:** membership refresh, focus pull, frame-callback selection and culling run every iteration or frame, whether or not the camera moved. A camera move only changes their outcome (which windows are in view, which enter/leave is sent).
   - **No configures:** DriftWM sends no configure to windows because of a pan.

### DriftWM — derived complexity (inference from the traced loops above)
| Path | Cost per execution | How often |
| :--- | :--- | :--- |
| Camera value update (pan event, momentum or zoom tick) | O(1) + O(O); + O(P·W) when pinned windows exist | per input event or animation frame |
| `compose_frame` cull loop | O(W) box tests + O(W) linear lookups of cost O(W) each = **O(W²)** comparisons worst case; drawing work O(V) | per rendered frame, per output |
| `post_render` frame-callback loop | O(W²) by the same lookup | per rendered frame, per output |
| `refresh_window_outputs` | O(W·O) overlap tests + O(W²) from `position_of` | every event-loop iteration **and** every `post_render` |
| Pointer focus pull (`focus_cascade`) | a constant number of O(W) front-to-back walks, early exit on a hit; full O(W) over empty canvas | every event-loop iteration and every rendered frame |
| `auto` placement | O(W²) cluster growth + slot tests against all windows | once per new window |

The O(W²) terms come from `Vec` scans comparing window handles. That is cheap per comparison, and at the window counts DriftWM targets the absolute cost is unknown. The asymptotic shape is source-derived; no magnitude is.

### DriftWM — a membership/culling mismatch at zoom < 1 (inference, not executed)
`refresh_window_outputs` (`src/state/membership.rs:51–109`) tests each window against `Space::output_geometry`. That rect is the camera-origin rectangle of **output-logical size** (fact 3) and ignores zoom. The render cull and the frame-callback choice use the zoom-aware `visible_canvas_rect`. At zoom < 1, a window drawn in the part of the viewport beyond the zoom-1 rectangle would therefore be on screen and receive full-rate frame callbacks, yet get no `wl_surface.enter` for that output. I found no compensating path in `membership.rs` or `update_output_from_camera`. This was not observed at runtime.

### Infynitae (main c6d47c9), for comparison
1. **Camera.** One camera per output with a Q16 `i64` **centre** and an `f64` zoom (`crates/infynitae-core/src/navigation.rs`; `ViewParams`, `projection.rs:17`). Mutation needs an `Authorized` proof (`set_center`, `navigation.rs:171`; INV-CAMERA-1). The zoom domain is any finite positive value; persistent zoom ≠ 1 is gated by OB-24.
2. **Transform.**
   - `View::new` (`projection.rs`, F-VIEW-1, F-LATTICE-1) computes the view origin `v = Q16(centre − view/(2k))`, `k = zoom·scale`.
   - It snaps the camera to the physical grid through a 120 px lattice (`c`, `c_eff`).
   - Integral canvas edges are projected by `edge(x) = round((x − R)·k + δ) − C` without separate rounding of origin and size (`physical_rect`).
   - Unprojection is `q = c_eff + Q16(p/zoom)` (`unproject`, F-UNPROJECT-1).
   - Rounding sites are the enumerated R1–R8 (INV-COORD-2).
3. **Pan.**
   - `pointer_motion` with `Interaction::CameraPan` (`crates/infynitae-core/src/entry.rs:211–231`) sets the camera to `start − Q16(sum)`: the f64 sum of the **whole gesture**, quantised once (F-PAN-1, `navigation.rs:160–189`, INV-COORD-3).
   - It drops that output's cached frame and runs `visibility_pass_camera` (`lib.rs:504`).
   - No motion reaches a client during the pan (INV-INPUT-3). The pan ends with one true-position event (INV-CAMERA-4).
   - There is no momentum and no camera animation.
4. **Culling.** `ProjectionFrame::build` (`render.rs:104–110`) walks the z-order (`Stage::front_to_back_boxed`, `stage.rs:196`). Each step does one `binary_search` lookup (`stage.rs:215–216`), skips elements whose canvas box cannot meet the view (`View::may_show`), and projects surfaces only for the rest (`entries_culled`, `render.rs:80`).
5. **Hit-testing.**
   - `Core::hit_test` (`lib.rs:565–580`) selects on the output's `ProjectionFrame`, rebuilding it first if a pan dropped it.
   - `ProjectionFrame::select` (`render.rs:126–150`) scans only the frame's culled entries front to back (F-HIT-1).
   - Focus is re-derived at Core chokepoints, not by a per-iteration pull (KNOW-RES-009).
6. **Membership.** `MembershipTable::compute` / `with_output` (`visibility.rs:60`, `:133`) derive enter/leave from the **same** `View` and the same `entries_culled` as the render list (F-MEMBER-1). Membership and rendering cannot disagree about which outputs show a surface, at any zoom.
7. **Placement.** F-PLACE-1 (`crates/infynitae-core/src/protocol.rs:121`, `:854`):
   - The provisional anchor at initial commit is the camera centre of the lowest-id output.
   - At the first windowed presentation, the window geometry is centred in the usable region, or origin-aligned if larger, with one R3 rounding: O(1).
   - It is defined for zoom 1 only.
   - There is no cluster or auto placement.
8. **Per-window work on a camera move.** No element's canvas geometry changes (INV-CAMERA-2), and no configure is sent (INV-CAMERA-4). The visibility pass re-tests every mapped element's box against the one changed view: O(W) box tests, O(S_V) surface projections, and a sort and diff of the membership rows, O(R log R). Measured T0 on the culled model (`85a74cb`): pan 23.5 µs and render list 101 µs at 3,000 elements (CHECKPOINT).

### Complexity comparison (asymptotic only)
| Question | DriftWM 352333a | Infynitae c6d47c9 |
| :--- | :--- | :--- |
| Camera value update | O(1) (+O(O) output remap, +O(P·W) pinned) | O(1) |
| Work triggered by a pan update | none beyond the above; the next iteration/frame does the periodic work | O(W) box tests + O(S_V) + O(R log R) membership, on the panned output only |
| Periodic work without any camera change | O(W²) per frame (render + post-render lookups), O(W²+W·O) membership per iteration, O(W) focus pull per iteration | none: membership and focus are recomputed only at chokepoints (map, commit, pan, output change) |
| Render-list cull | O(W²) lookups + O(W) box tests | O(W log W) lookups + O(W) box tests + O(S_V) |
| Hit-test | O(W) worst case, over all stage entries | O(entries in the culled frame) ≤ O(S_V) |
| Initial placement | O(1) seed; `auto`: O(W²) | O(1) |

**Overall panning is not constant-time in either compositor.** In both, the camera value changes in O(1), but the frame that follows costs at least O(W) to cull. DriftWM's documentation describes the camera model; nothing in its source makes the post-pan work independent of W.

## Unverified Hypotheses
- **Magnitudes.** The source supports the asymptotic shapes above and nothing more. No DriftWM benchmark exists at `352333a` (no `benches/`, no criterion). Infynitae's T0 numbers come from a different scenario, so the two cannot be compared as measured costs.
- **Proposed minimal benchmark (not implemented; needs owner approval).**
  - **Setup:** one machine and one `rustc`, release profile, CPU governor recorded, headless only.
  - **Infynitae:** time `pointer_motion` during `CameraPan` and `hit_test` over empty canvas, with W ∈ {10, 100, 1000, 3000} and the viewport covering about 10 windows.
  - **DriftWM:** time one `drift_pan_on` plus one `refresh_and_flush_clients` iteration, and one `focus_cascade` over empty canvas, with the same populations. Use its in-process test `Fixture` (`src/tests/fixture.rs:25`), which needs no GPU. `compose_frame` needs a GLES renderer and is excluded.
  - **Where the timing code lives:** in a scratch copy of DriftWM outside the Infynitae repository, never committed there (GPL).
  - **Purpose:** check the predicted W² against linear growth. The two absolute numbers must not be compared, because the work measured differs (DriftWM's iteration includes client flushing and protocol work).
- **Zoom membership.** The zoom < 1 membership mismatch is an inference from source. A two-window wire probe at zoom 0.5 would confirm or refute it.

## Performance & Correctness Implications
- **What Infynitae already does better** (the culled `ProjectionFrame`, binary-search `Stage`, chokepoint re-derivation, one shared `View` for render, membership and hit-test): it scales better asymptotically than DriftWM's per-iteration pull. No change is suggested.
- **What DriftWM handles that Infynitae has not specified yet:**
  - camera animation (momentum, animated zoom), with input tracking the live camera and the pointer kept screen-fixed by warping its canvas location;
  - zoom-to-fit and dynamic minimum zoom;
  - placement relative to existing windows (`auto`).

  These are product features, not performance findings. Any of them would need its own owner decision and, for camera animation, a per-step authorization (INV-CAMERA-1).
- **Where Infynitae differs by design:** DriftWM accumulates the camera per event in `f64`, while Infynitae re-derives from the gesture origin and quantises once (INV-COORD-3). That rule exists to prevent drift that per-event accumulation allows; no change is suggested.

## Limitations & Counterexamples
- Only the files and direct callees of the eight questions were read. Grabs other than the pan grab, touch and gesture paths, the background renderer and the udev backend were not traced.
- The complexity statements cover the code paths named. A path not traced could add work, for example `window_cull_rect` (`src/state/window_animation_driver.rs:667`, read only at its signature).
- DriftWM's Smithay is a git revision (`4cf0b62`), not the `=0.7.0` Infynitae pins. Smithay behaviour cited here (`output_geometry`) is from that revision.

## Reproduction Instructions
```bash
D=~/.gemini/antigravity/scratch/driftwm
git -C $D rev-parse HEAD   # 352333a8fa1b22171492d4b71a54102045c9a19d
sed -n 146,151p $D/src/stage/mod.rs                    # Stage::entry is a linear find
grep -n 'position_of\|id_of\|for element in state.stage.windows' $D/src/render/mod.rs | head
sed -n 249,276p $D/src/render/lifecycle.rs             # post_render window loop
sed -n 1276,1295p $D/src/state/mod.rs                  # per-iteration membership + focus pull
sed -n 384,400p ~/.cargo/git/checkouts/smithay-*/4cf0b62/src/desktop/space/mod.rs
git show main:crates/infynitae-core/src/render.rs | sed -n 104,150p
git show main:crates/infynitae-core/src/stage.rs | sed -n 196,216p
```

## Links & Citations
- Plan and earlier summary: [KNOW-RES-009](KNOW-RES-009-driftwm-camera-viewport-and-hit-testing.md); idea register: [KNOW-RES-010](KNOW-RES-010-driftwm-idea-disposition-register.md)
- Infynitae culling history: [KNOW-ARCH-004](../architecture/KNOW-ARCH-004-spatial-membership-culling.md); focus pull: [KNOW-PITFALL-002](../pitfalls/KNOW-PITFALL-002-global-focus-pull-loops.md)
- Ledger: [CONTRADICTIONS_AND_SUPERSEDED.md](../CONTRADICTIONS_AND_SUPERSEDED.md) G-01, X-08
