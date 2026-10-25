# INC-3 evidence — pointer intake, ownership, CameraPan at zoom 1, routing, zero outputs

- **Category / execution class:** PRODUCT, HEADLESS.
- **Baseline:** `6470e1a` (INC-0/1/2 ratified).
- **Evidence files** (captured on the implementation tree):
  - [`IMPLEMENTATION_MAPPING.md`](IMPLEMENTATION_MAPPING.md): written before any production change;
  - [`test-matrix.txt`](test-matrix.txt);
  - [`verify-architecture.txt`](verify-architecture.txt);
  - [`verify-test-efficacy.txt`](verify-test-efficacy.txt);
  - [`verify-development-environment.txt`](verify-development-environment.txt).

**Status: INC-3 IMPLEMENTED — PENDING INDEPENDENT REVIEW.**
- No obligation was closed.
- No canonical architecture, invariant, formula or audit record was changed.

## 1. Canonical INC-3 (FOUNDATION_IMPLEMENTATION_PLAN)

- **Objective:**
  - `PhysicalEvent` intake through `InputPort`;
  - the ownership table;
  - the interaction state machine (M22, CameraPan only);
  - pointer routing with F-HIT-1;
  - KeepFocus;
  - free-pointer reconciliation.
- **Modules:**
  - core: `input::{ownership, interaction, bindings}` and `navigation` pan (F-PAN-1, zoom 1);
  - adapter: pointer delivery;
  - headless: injected input.
- **State:** OBJ-20, OBJ-25, OBJ-26.
- **Seam:** `wl_pointer`, relative pointer.
- **Invariants:** INV-INPUT-1/2/3/4, INV-COORD-3, INV-COORD-5, INV-CAMERA-2, INV-CAMERA-4.
- **Obligations:**
  - OB-4 (acquisition part);
  - OB-21 (T0 part);
  - OB-45 (hit-test part);
  - OB-50 (pointer part);
  - OB-3 (interaction orphaning, per CHECKPOINT).
- **State machines:** M6 (zoom-1 rows), M10, M22.
- **Formulas:** F-PAN-1, F-HIT-1, F-UNPROJECT-1, F-RELOCATE-1, F-RECONF-1, B2.5 T1 (Arrangement motion).
- **Exit tests:**
  - TST-05 x8 (constraint cases C3–C5 belong to INC-5);
  - zero configures during pan;
  - EE-1 C5 on Normal/90/180/270 (+ flipped);
  - the zero-output transition (EE-3, no constraint);
  - the INV-INPUT-3 bypass switch must fail.

The coordinate domains, state ownership, non-goals and judgment calls are in `IMPLEMENTATION_MAPPING.md`.

## 2. Production path

```
headless backend (virtual mouse)  infynitae_backend_headless::{pointer_motion, pointer_button, pointer_axis}
  → Core::physical_input(&mut InputPort, PhysicalEvent)           (port bound to its Core; non-finite rejected)
      P1 ownership: OBJ-25 Ownership (decided at press), OBJ-26 Interaction
      P2 binding: BTN_MIDDLE + Logo, only when the client owns no button and the camera zoom is 1
      CameraPan motion: Authorized minted here → CameraSet::pan (F-PAN-1) → frame dropped → visibility pass
      client path: pointer::moved (Arrangement, union clamp) → Core::route_pointer → Core::hit_test (ProjectionFrame, F-HIT-1)
  → Vec<Effect> (PointerMotion / PointerRelative / PointerButton / PointerAxis / PointerFrame /
                 PointerZeroOutputBegin / PointerZeroOutputEnd, plus Enter/Leave)
  → WaylandState::apply → infynitae_wayland::pointer (the only PointerHandle caller)
  → Smithay PointerHandle (global space = Canvas) → wl_pointer / zwp_relative_pointer_v1
```

- The headless `vblank` now produces the output's frame before the frame clock ticks. It records the ProjectionFrame, as a real backend does, so the hit test reads what was shown (INV-HIT-1).
- Before this change, a frame was built only on demand, and a frame built while nothing was mapped would have persisted.

## 3. Smithay 0.7.0 source findings (pinned registry source)

| # | API / behaviour | Source | Consequence |
|---|---|---|---|
| S1 | `PointerHandle::motion(focus, ev)`: the client receives `location − focus origin`. Same focus ⇒ `motion`; a new focus ⇒ `leave` + `enter`; `None` ⇒ `leave` | `input/pointer/mod.rs:785-818` | The focus origin and the location are both Canvas (D-B2-PTR-SPACE). The adapter passes `origin + hit.local`, so the client receives F-HIT-1's clamped coordinate exactly. |
| S2 | `relative_motion` is delivered to the **current** focus; the `focus` argument only sets `pending_focus` | `mod.rs:820-830`, `wayland/seat/pointer.rs:271` | Core emits it with the same routing as the motion, to keep `pending_focus` coherent. |
| S3 | `button()` updates `pressed_buttons` **before** the grab sees the event; `DefaultGrab` starts a `ClickGrab` on press; `ClickGrab` ends only when `pressed_buttons` is empty | `mod.rs:271-289`, `grab.rs:233-250, 377-381` | A client-owned release in NoOutput is forwarded into the zero-output grab, which delivers nothing. Otherwise a stale pressed button would keep a later click grab alive (tested; control). |
| S4 | `set_grab(.., Focus::Clear)` clears the focus through an internal `motion(None)` ⇒ `leave` (+ `frame` for wl_pointer ≥ 5) and constraint deactivation | `mod.rs:748-760`, `seat/pointer.rs:84-90, 246-256` | The zero-output transition sends exactly one leave at removal. |
| S5 | `unset_grab` restores focus by a direct inner `motion` to `pending_focus` at the stored location | `mod.rs:763-783` | The adapter sends `motion(None)` after installing the zero-output grab. Otherwise a reconnect would re-enter the old surface at a stale location (fabricated history; tested; control). |
| S6 | `wl_pointer.frame`/`button`/`axis` go only to the focused surface | `mod.rs:603-627` | With no focus, nothing is sent. |
| S7 | A `leave` whose surface the client destroyed is not observable; the following `frame` is (x8 C9) | observed on the wire | Recorded in the C9 test. |
| S8 | wl_fixed encoding is `(v * 256.) as i32` (truncation) | wayland-scanner 0.31.11 `common.rs:523` | Test oracle `wl_fixed()`. |
| S9 | A `wl_seat` global needs `delegate_seat!`; `RelativePointerManagerState::new` creates `zwp_relative_pointer_manager_v1` | `wayland/seat/mod.rs`, `wayland/relative_pointer.rs` | Seat `seat0` with a pointer and no keyboard (INC-4). |
| S10 | `Window::surface_under` and `Space` are not used | — | The existing `smithay-rejected-apis` rule is unchanged. |

## 4. Authority analysis

- **Minting.**
  - The camera proof for a pan is minted in `physical_input` itself, once per motion owned by a pan (`MINT_ENTRIES` unchanged).
  - A first draft minted in a helper; the existing `mint-location` rule rejected it, and the mint was moved.
- **`core::input`:**
  - decides only;
  - cannot mint (`pub(in crate::entry)`);
  - holds no `&mut CameraSet` or `&mut Stage`.
- **`CameraSet::pan`:** `pub(crate)`, and it consumes the proof. `set_center` stays private.
- **New source rules** (with seeded controls):
  - `effects-only-from-core`: product crates other than core never construct `Effect` values. Patterns are allowed; renamed or variant imports and macro bodies are checked. Three seeds.
  - `smithay-pointer-only-in-adapter-pointer`: `motion`, `relative_motion`, `button`, `axis`, `frame`, `set_grab`, `unset_grab` and `get_pointer` are called only in `infynitae-wayland::pointer` (method, qualified and macro forms). Two seeds.
- **New `mint-location` seeds:** a mint in a `physical_input` helper, and the input module panning with its own proof.
- **New compile probes:**

  | Probe | Expected diagnostic |
  |---|---|
  | p18: hit test with a Canvas position | E0308 |
  | p19: a Canvas value as a device delta | E0308 |
  | p20: pan from outside core | E0624 |
  | p21: place the pointer from outside core | E0603 |

- **Runtime controls (`verify-architecture` [5c]):**
  - `probe_pan_motion_bypass` (the INV-INPUT-3 bypass switch) makes the T0 and T1 KeepFocus tests fail with "INV-INPUT-3 violated".
  - `physical_input` port binding is tested at runtime. The DEFERRED list keeps only `shell_command` (INC-11).

## 5. Tests (all on the production path; oracles hand-derived or client-observed)

- **T0 core: 66** (13 new).
  - **Intake:**
    - `client_motion_is_routed_with_its_relative_delta`;
    - `a_pan_emits_no_client_effect_and_ends_with_one_true_position`;
    - `pan_quantises_the_gesture_sum_once_per_update` (10⁵ events; per-event rounding drift is the control);
    - `pan_at_fractional_scale_moves_by_the_logical_delta`;
    - `ownership_is_decided_at_press`;
    - `no_output_consumes_everything`;
    - `output_removal_orphans_the_pan`;
    - `a_pan_drops_the_output_frame`.
  - **Pure modules:**
    - `pan_target_quantises_the_sum_once`;
    - `locate_clamps_to_the_union_of_output_rects`;
    - the ownership, interaction and binding unit tests.
  - **Updated:** three INC-2 tests whose exact effect lists now include the M10 pointer effects, with hand-derived values.
- **T1 lockstep:**
  - **`t1_input` (16):**
    - x8 C1, C6, C7, C8, C9 (C2 is the rejected LeaveFocus mode; C3–C5 are INC-5);
    - routed enter + relative;
    - release before the next frame;
    - membership pan with 0 configures;
    - OB-24 zoom gate;
    - EE-3 with a held button and with a free pointer;
    - orphaned pan;
    - reconfiguration;
    - Arrangement crossing;
    - axis;
    - intake rejections.
  - **`t1_adversarial` (12):** attacks A–M (G and H are also covered in `t1_input`).
  - **`t1_transform` (+2):** `ee1_c5_render_hit_agreement_on_the_production_path` (10 cases × 200 points, 0 mismatches; the sealed hit rule fails 9/9 non-Normal cases) and `n_pan_moves_render_and_hit_together`.
- **Total:** 121 passing (66 T0; 49 distinct T1 tests plus the oracle self-test in each of 6 T1 binaries).

### Adversarial attacks (authorization §16)

| Attack | Test | Oracle |
|---|---|---|
| A pixel boundary | `a_physical_pixel_boundary` | first/last pixel and one wl_fixed step outside, both axes |
| B fractional scale | `b_fractional_scale_edge_and_clamp` (k = 1.5): last pixel 1411; the F-HIT-1 clamp delivers 300.99609375 | lattice values derived by hand |
| C negative Canvas | `c_negative_canvas_coordinates` (camera −5000, −3000) | c_eff by hand |
| D near the ±2³⁶ bound | `d_large_canvas_coordinates_near_the_bound`: routing at 2³⁶ − 2000; the pan saturates at exactly 2³⁶ | by hand |
| E output transition | `e_output_transition_uses_the_new_outputs_camera`; `motion_crosses_the_arrangement_and_clamps_to_the_union` | B's camera on the wire |
| F output removal | `f_output_removal_relocates_to_the_nearest_survivor` (valid target / no surface / near edge / 2 survivors; not first-in-order); orphaned pan; no survivor (EE-3) | wire + core state |
| G zero outputs | `zero_output_transition_*` | one leave at removal, nothing while NoOutput |
| H first output returns | `h_first_output_returns_without_history` (remembered camera, startup pointer, no old position) | wire |
| I stacking | `i_overlapping_surfaces_route_to_the_presented_one` | wire |
| J input-region holes | `j_input_region_holes_fall_through` | wire |
| K unshown / unmapped | `k_unmapped_surface_receives_nothing` (an unmapped toplevel; an unmapped parent subsurface with a still-mapped child) | wire |
| L stale frame after reconfiguration | `l_no_stale_frame_after_reconfiguration` (scale 1 → 2); T0 INC2-F07 | wire |
| M Stage unchanged by a pan | `m_pan_never_writes_the_stage` (anchor, state, tree, z-order) | byte-equal Debug snapshot |
| N render and hit move together | `n_pan_moves_render_and_hit_together` (Normal, 90°@1.25, fractional Canvas camera) | presented-pixel readback |
| O wrong coordinate domain | compile probes p18, p19 | rustc E0308 |

## 6. Mutation controls (`verify-test-efficacy`)

- **30 new INC-3 controls**, all DETECTED (76/76 controls in total, plus judge controls; canonical tree byte-identical):
  - wrong output at a boundary;
  - half-open clamp dropped;
  - nearest survivor replaced by the first;
  - x/y swapped in the location;
  - pixel rounded instead of floored (scale 1 and 1.5);
  - back-to-front routing;
  - input region ignored;
  - unshown surface interactive;
  - pan sign reversed;
  - scale omitted;
  - per-event quantisation;
  - a pan writes Stage anchors;
  - stale frame kept (T0) / render-but-not-hit (T1, release before the next frame);
  - no orphaning;
  - no true position at release;
  - relative motion synthesised;
  - acquisition with a client button;
  - presses during a pan delivered;
  - modifier ignored;
  - OB-24 gate removed;
  - axis during a pan;
  - `Focus::Keep`;
  - no zero-output grab;
  - pending focus kept (fabricated reconnect);
  - grab not removed;
  - client release dropped in NoOutput;
  - relocation not re-routed;
  - reconfiguration not re-routed.
- **Efficacy findings during this work** (fixed before claiming completion):
  - "unmapped surface stays interactive" first targeted Stage lifecycle. That mutant was equivalent, because unmap also drops the snapshot. It was replaced by the unshown-filter defect, and attack K was strengthened with an unshown subsurface that still has a buffer.
  - The `Focus::Keep` control was first MISSED, because the free-pointer test checked the whole removal-to-reconnect window, in which the late leave masked the defect. The test now asserts the wire at removal time.
- **Re-anchored INC-2 control:** "global removed before the leaves" was re-anchored to the new `output_disconnected` body (same defect; the pointer effects now sit between the leaves and `OutputRemoved`).

## 7. Obligations (recommendations; none closed)

| Obligation | INC-3 part | Recommendation |
|---|---|---|
| OB-45 | Hit-test part: EE-1 C5 reproduced on the production adapter, 0 mismatches over 10 cases; the sealed hit rule fails every non-Normal case | Pass criterion met for all parts (INC-2 tracker + INC-3 hit). **OPEN pending review.** |
| OB-50 | Pointer part: EE-3 no-constraint and held-button cases on the production adapter (leave at removal, nothing in zero state, enter after reconnect, no stuck grab) | Pointer part satisfied; OPEN (constraint part, INC-5) |
| OB-3 | Interaction orphaning (C4.6): ownership kept, motion consumed, reconciliation on the relocated output, no stuck buttons | Satisfied for CameraPan; OPEN (INC-5 constraint reconciliation) |
| OB-21 | T0: `C = start − Q16(Σ)`, no drift over 10⁵ events; per-event control | T0 part satisfied; OPEN (T4 libinput) |
| OB-4 | Acquisition precondition and ordering point exist. `xdg_popup.grab` is not honoured before INC-4, so no popup grab can exist at acquisition and `popup_done` dismissal cannot be exercised. | Not exercised; OPEN (INC-4) |
| OB-1, OB-6, OB-11, OB-14, OB-17, OB-48 | untouched | OPEN |

## 8. Findings and observations

| ID | Level | Statement | Disposition |
|---|---|---|---|
| INC3-F01 | observation | The modifier source for the pan binding is a per-event `Modifiers` snapshot supplied by the backend. Core keeps no copy (C4.4). There is no keyboard before INC-4, so the INC-4 adapter must fill it from `KeyboardHandle::modifier_state()`. | INC-4 |
| INC3-F02 | observation | A camera change drops the output's ProjectionFrame (the INC2-F07 rule extended). Until the next frame, hit tests use the list the next render will produce. | review |
| INC3-F03 | observation | The headless `vblank` now renders the output's frame (records the ProjectionFrame) before ticking the clock. | review |
| INC3-F04 | observation | D-B6-BOUNDARY rule 1 (the popup's input output) is not implemented. M3 is not in the INC-3 objective, and its grab-serial half needs INC-4 popup grabs. This corrects INC2-F02 ("waits for INC-3"). | INC-4 / owner |
| INC3-F05 | observation | The adapter snapshot gives an unmapped subsurface the offset (0,0), because the offset is read from the renderer view. Its subtree is unshown, so neither rendering nor input is affected. Attack K covers it. | none |
| INC3-F06 | observation | `verify-test-efficacy` shares `target/test-efficacy` between runs. Two concurrent runs (for example, `verify-development-environment` started during a manual run) can race on the test binaries and report a spurious MISSED. All recorded runs here were sequential. | tooling (review) |
| INC3-F07 | observation | x8 C9: after the focused surface is destroyed, reconciliation sends a lone `frame` (S7). | none |
| INC3-F08 | observation | Persistent zoom ≠ 1 (persisted cameras) refuses pan acquisition, and the press follows the client path (OB-24 gate). | OB-24 |

**Open findings:** Level A 0, Level B 0, Level C 1 (INC1-F01, INC-7). INC-3 adds no Level A/B/C finding.

## 9. Zoom compatibility (not implemented)

- **Not done in INC-3:** no zoom was implemented; zoom ≠ 1 never coexists with a pan (OB-24).
- **Pan already uses the zoom:** F-PAN-1 reads `z_C` from the camera.
- **What a future zoom about the pointer needs:**
  - Keep `Q = unproject(O, p)` fixed: `centre' = Q − (p − view/2)/z'`.
  - It is a camera-only change: the pointer stays `(O, p)`, and neither output scale, the Stage nor the Arrangement is involved.
  - It must also rebuild the damage tracker (INC2-F06).

## 10. Safety

- **Execution:** headless only. No DRM/KMS, no VT switch, no package change, no live compositor, the old Arch partition untouched, no unsafe.
- **Pins:** Smithay stays `=0.7.0`; no `[patch]`; P1 not applied.
- **INC-4:** not started.
