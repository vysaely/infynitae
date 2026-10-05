# Infynitae Native — Foundation Evidence (Phase A)

Status: **Phase A artifact.** Written 2026-10-04. Every claim below is classified; nothing here is
an architecture decision. Decisions belong to Phase B (`FOUNDATION_BLOCKERS.md`) and later.

Companion documents:
- [PROTOTYPE_DECISION_LEDGER.md](PROTOTYPE_DECISION_LEDGER.md): per-mechanism REUSE/ADAPT/… classification of Prototype 01 (A2).
- [CHECKPOINT.md](CHECKPOINT.md): current phase, next action.
- Raw evidence: [`evidence/phase-a/`](evidence/phase-a/).

---

## 0. How to read this document

**Classification** (per master prompt §5): `VERIFIED FACT`, `PROJECT REQUIREMENT`, `DESIGN DECISION`,
`ASSUMPTION`, `OPEN HYPOTHESIS`.

**Evidence type**: `REPO` (repository evidence), `PROTO-MEASURE` (prototype measurement / recorded run log),
`SMITHAY` (Smithay 0.7.0 source), `SPEC` (Wayland protocol XML), `ESTABLISHED` (other-compositor
behavior, policy precedent only), `EXPERIMENT` (controlled experiment run for this program).

**"VERIFIED (static)"** means verified by reading source, not by observing a running system. That is weaker
than a protocol trace and is labeled as such wherever it matters.

**No controlled experiments were run in Phase A.** No nested compositor was started. The only things
executed were the prototype's unit tests and its `--verify` mode, which is pure arithmetic and opens no
window or socket.

---

## 1. Sources inspected

| Source | Identity | Notes |
|---|---|---|
| Hyprland Infynitae | `~/infynitae`, branch `dev/infynitae-shell`, HEAD `125f8de` | `src/infynitae.cpp` (5,889 LOC), `docs/ARCHITECTURE.md`, `docs/history/research/*.md`, `docs/history/2026-10-integration-audit/ARCHITECTURE_OPTIONS.md`, `HOOK_RISK_REGISTER.md` (skimmed) |
| Prototype 01 | `~/infynitae-native-prototype`, **no commits** (all files untracked) | All 1,303 LOC read in full. SHA-256 of every file inspected: [`evidence/phase-a/prototype-source-hashes.txt`](evidence/phase-a/prototype-source-hashes.txt) |
| Prototype 01 run logs | `~/.gemini/antigravity-cli/brain/0a6f9e0b-…/.system_generated/tasks/task-{280,316}.log` and session transcript | Logs from the agent session that built the prototype. Excerpts: [`evidence/phase-a/prototype-runtime-logs.txt`](evidence/phase-a/prototype-runtime-logs.txt) |
| Smithay | crates.io `smithay 0.7.0`, checksum `740cea69…af98`, local source `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/smithay-0.7.0` | [`evidence/phase-a/smithay-pin.txt`](evidence/phase-a/smithay-pin.txt), [`smithay-0.7.0-excerpts.txt`](evidence/phase-a/smithay-0.7.0-excerpts.txt) |
| Wayland protocols | `wayland 1.26.0`, `wayland-protocols 1.49` (system XML under `/usr/share`) | Verbatim excerpts: [`evidence/phase-a/wayland-protocol-excerpts.txt`](evidence/phase-a/wayland-protocol-excerpts.txt) |
| Toolchain | rustc 1.99.0, cargo 1.99.0 | |

---

## 2. Corrections to the master prompt's premises

The master prompt (§4) lists what Prototype 01 "already demonstrated experimentally". Checked against
code and logs:

| # | Prompt premise | Finding | Ref |
|---|---|---|---|
| 1 | "f64 Canvas coordinates" (A2 list) | **False.** Prototype Canvas, Camera and projection are `i32` (`spatial.rs:16-19, 84-87`). The *Hyprland* plugin uses `f64` (`infynitae.cpp:109-110, 176-177`). | EV-P01, EV-H04 |
| 2 | "Camera pan can cause zero client configure events" | **Not measured.** `--verify` prints `Configure traffic during pan: 0` from hard-coded literals (`verification.rs:81-82`). No recorded run ever exercised pan. Static analysis supports the claim; no protocol trace exists. | EV-P02, EV-P03, EV-P04 |
| 3 | "Camera pan can cause zero world-coordinate mutations" | **True by static analysis.** The verify-suite assertion is vacuous: it compares a local constant with itself (`verification.rs:50-59`). | EV-P05 |
| 4 | "independent conceptual output cameras are possible" | **Only arithmetically.** The prototype has exactly one `Camera` and one `Output` (`state.rs:42`, `winit.rs:29`). "Multi-viewport" is two `Camera` values projecting one point in a unit function. | EV-P07 |
| 5 | "Hit testing can inverse-project pointer coordinates" | **Implemented, but inconsistent with rendering and with Smithay's pointer contract.** Clients get wrong surface-local coordinates (§5 EV-P08, EV-P09). | EV-P08, EV-P09 |
| 6 | "Pan can isolate pointer motion from clients" | **Partially.** During pan, `wl_pointer` focus is cleared, which itself sends `wl_pointer.leave`. Relative motion is never sent *at all*, pan or not, so no isolation of relative motion was tested. | EV-P15 |
| 7 | "ordinary Wayland clients can run nested" | **True for mapping.** kitty (Wayland) mapped twice. Interaction correctness is unverified (see #5). Popups do not work. | EV-P20, EV-P12 |
| 8 | "xwayland-satellite can provide X11 compatibility" | **True for startup and one mapped X11 toplevel** (satellite 0.8.3, X11 kitty). | EV-P18 |
| 9 | "hardware GLES rendering works" | **True** (AMD RX 9070 XT, Mesa 26.2.3, GLES 3.2 via winit/EGL). | EV-P19 |
| 10 | "Smithay's Space does not need to own window placement" | **True** (Space not used). But enter/leave, which Space normally drives, was then never implemented. | EV-P21, EV-P13 |
| 11 | "~1,300 LOC" | True (1,303). It omits destroy handling, popups, enter/leave, scale, constraints and relative motion. LOC is not comparable to a foundation. | EV-P11…P15 |

The prototype's own final report (session transcript, step 335) states "Confirmed 0 configure events
generated during pan" and "Verdict: PASS … Confidence 96%". **That confidence is not supported by the
recorded evidence.** This is exactly the hazard named in AI3.

---

## 3. A1 — Current Hyprland Infynitae: what survives, what disappears

### 3.1 Product semantics that should survive (PROJECT REQUIREMENT, mostly Level D for Foundation 01)

From `README.md`, `docs/ARCHITECTURE.md`:
- One continuous canvas with persistent per-window world coordinates; camera panning (`Super+MMB`).
- **During pan, pointer-locked games receive no motion** (README "What it does").
- **Camera jumps keep the cursor at the same screen position** (ARCHITECTURE §4.3).
- Snapping and persistent snap clusters (Level D).
- **Viewport fit is distinct from native client fullscreen** (ARCHITECTURE §4.1 vs §4.2). Both preserve
  world coordinates.
- Native fullscreen is visually exclusive: other canvas windows never draw over it (§4.2).
- Overview with **live thumbnails of off-screen windows** (§5). This needs off-screen content without
  unthrottling off-screen clients (offscreen-thumbnails.md §1 "Semantic Requirement").
- Slots, bookmarks, recenter (Level D). Capture privacy, `no_screen_share` (Level C/D). Scriptable
  control socket plus event socket (shell boundary, C7).
- Zero idle cost: "no threads, timers or polling" (ARCHITECTURE §0).

### 3.2 Mechanisms that exist only because of Hyprland (DISCARD)

| Mechanism | Location | Why it disappears |
|---|---|---|
| Pan rewrites every window's compositor position (`moveWindowTo` per window per motion event; O(N)) | `infynitae.cpp:4556-4614` (`panRelative`, `updateWindowPositions`) | Hyprland owns monitor-space geometry. Native Stage + projection removes it (I2). |
| Detours on `onMouseMoved` / `onMouseButton` to consume pan input | ARCHITECTURE §6 Hook 1/2 | Native compositor owns input routing. |
| Render-pass constructor detours to hide borders / suppress windows under fullscreen | Hook 3/4 | Native renderer owns render-element selection. |
| Forcing windows floating at `openEarly`, suppressing tiling layouts/workspaces | ARCHITECTURE §2.2 | No foreign layout model. |
| ABI policy, compat boundary, fault latching, adapter, patchset | `src/compat/`, `hyprland-adapter/` | No foreign ABI. |
| Off-screen screencopy patch (cull-gate bypass) | `offscreen-thumbnails.md` §3 | Native compositor owns capture scheduling directly. |
| Cursor warp-back after camera jumps | ARCHITECTURE §4.3 | Native: the camera moves and the pointer's OutputLogical position is untouched. Invariance becomes structural, no repair needed. **Still a requirement**, now satisfied by design rather than repair. |
| Pointer-address string identity | `formatAddress(void*)` `infynitae.cpp:4866-4870`, keys of `windows`, `SnapEdge` | Violates I7; replaced by compositor-owned IDs. |
| Single "canvas monitor" heuristics | `getCanvasMonitorInfo` `infynitae.cpp:304-` | Replaced by per-output cameras (B7). |

### 3.3 Lessons from Hyprland-era research that carry over (ESTABLISHED in Hyprland, policy input only)

From `docs/history/research/pointer-input-findings.md` (Hyprland behavior; **not** evidence about Smithay):
- Absolute-pointer games (Dota 2) break when given out-of-surface coordinates. Clamping the cursor to
  the viewport edge triggers in-game edge-pan zones.
- Relative-pointer clients (CS2) and absolute clients need different treatment. One generic
  "cursor preservation" policy caused mutual interference.
- Pointer focus, keyboard focus, constraint eligibility and canvas visibility are **orthogonal**.
  Conflating them caused constraint re-activation loops. This directly informs B10/C4.
- Off-screen games in Hyprland received **zero frame callbacks** and stayed idle. The product
  requirement wanted exactly that (offscreen-thumbnails.md §2.5, §1). See EV-W02 for why this collides
  with the master prompt's anti-starvation rule.

---

## 4. A3 — Smithay pin

- **Pinned:** `smithay = "0.7.0"` from crates.io, `Cargo.lock` checksum
  `740cea6927892bc182d5bf70c8f79806c8bc9f68f2fb96e55a30be171b63af98`.
- **Newest stable on crates.io as of 2026-10-04:** 0.7.0 (published 2025-06-24). The pin is therefore
  current-stable but **about 15 months old**. Smithay git `master` has certainly moved; how far, and whether
  it fixes anything relevant to us, was **not investigated** (OPEN, see §7 Q-07).
- **Features used by the prototype** (`default-features = false`): `backend_winit`, `backend_egl`,
  `renderer_gl`, `wayland_frontend`, `desktop`. **`xwayland` is not enabled**. It is not needed with
  xwayland-satellite, which is an ordinary Wayland client.
- Future production features (not yet verified to build): `backend_drm`, `backend_gbm`,
  `backend_libinput`, `backend_udev`, `backend_session_libseat`. Names verified in the
  0.7.0 `[features]` block.
- Transitive pins: `wayland-server 0.31.14`, `wayland-protocols 0.32.13`, `calloop 0.14.5` (and 0.13.0),
  `winit 0.30.13`.
- **Pinning/update strategy:** not decided. Phase B decision (Level B), input in Q-07.

APIs relied on, verified to exist in 0.7.0: see EV-S03 … EV-S11.

---

## 5. Evidence matrix (A5)

Format per entry: **Claim** · Classification · Evidence type · Location · Confidence · Falsified by ·
Implication.

### 5.1 Prototype 01 (EV-P)

**EV-P01** · Prototype Canvas/Camera/projection are `i32`, not `f64`.
VERIFIED FACT · REPO · `spatial.rs:16-19, 62-67, 84-87` · HIGH · n/a (direct reading) ·
B1 has **no prototype evidence for f64**. The numeric model must be decided from first principles.

**EV-P02** · `--verify` "configure traffic during pan: 0" and "world mutations during pan: 0" are hard-coded literals, not measurements.
VERIFIED FACT · REPO · `verification.rs:81-82`; output in `evidence/phase-a/prototype-test-and-verify-output.txt` · HIGH · n/a ·
The prototype's headline I3 evidence is void. I3 needs an external, client-side measurement (C11).

**EV-P03** · No recorded prototype run exercised camera pan or printed final runtime stats.
VERIFIED FACT (for recorded runs) · PROTO-MEASURE · 0 of 6 task logs contain `Camera Pan START` or `Final stats` (`prototype-runtime-logs.txt`) · MEDIUM (unrecorded manual runs may have occurred) · A preserved log or trace showing a pan ·
Treat every pan-related prototype claim as unmeasured.

**EV-P04** · The prototype's pan path cannot send `xdg_toplevel.configure`.
OPEN HYPOTHESIS, strongly supported by static analysis · REPO + SMITHAY · Pan branch `input.rs:175-199` touches only `camera` and pointer focus. The only `send_configure` calls are `handlers.rs:76,114` (initial configure). Smithay's own `send_configure` calls in `shell/xdg/mod.rs:1145,1153` are default handlers for client `maximize`/`fullscreen` requests · MEDIUM-HIGH · `WAYLAND_DEBUG=1` client trace showing `xdg_toplevel.configure` during a pan with no concurrent client request ·
Likely true for this prototype, but irrelevant to production unless re-measured. Production adds states (`activated`, `suspended`, `bounds`) that could couple to visibility (EV-W05, EV-W06).

**EV-P05** · Pan mutates no Stage element position.
VERIFIED (static) · REPO · `Stage::set_element_canvas_pos` (`spatial.rs:195-200`) and `get_element_mut` (`:189`) have no callers. Note `StageElement.canvas_pos` is a `pub` field, so mutation is not encapsulated · HIGH · n/a ·
Supports I2. The verify-suite assertion meant to prove it is vacuous (`verification.rs:50-59`).

**EV-P06** · Camera mutation is O(1).
VERIFIED (static + trivial microbenchmark 8 ns) · REPO + PROTO-MEASURE · `spatial.rs:98-101` · HIGH · n/a ·
Supports I2. Says nothing about frame cost (C14).

**EV-P07** · Prototype has one Camera and one Output. "Multi-viewport" exists only as arithmetic on two `Camera` values.
VERIFIED FACT · REPO · `state.rs:41-42`, `winit.rs:29-46`, `verification.rs:33-46` · HIGH · n/a ·
**I5 and B8 (multi-output semantics) have zero empirical support.**

**EV-P08** · Clients receive wrong surface-local pointer coordinates.
VERIFIED (static) · REPO + SMITHAY · Smithay contract: the focus point is "the coordinates of [the surface's] origin in the global compositor space" (`input/pointer/mod.rs:225-227`), and clients receive `event.location - loc` (`:795`). The prototype passes the surface's offset *within the window* (`state.rs:182-187`, from `Window::surface_under`, `desktop/wayland/window.rs:330-336`: "point should be relative to (0,0) of the window"). Example: window at screen (80,80), pointer at (100,100) → client receives ≈(100,100) instead of (20,20) · HIGH · Client trace showing correct surface-local coordinates ·
**I9 as implemented is not met.** The production design must fix which space "global compositor space" means for Smithay's single pointer location. See EV-S08.

**EV-P09** · Rendering and hit-testing use different element origins.
VERIFIED (static) · REPO · Render places the **surface origin** at `canvas_pos - camera` (`render.rs:34, 54-58`). Hit-test treats `canvas_pos + geometry.loc` as the **window-geometry origin** (`spatial.rs:232-243`), then passes geometry-relative points to an API expecting surface-relative points · HIGH · n/a ·
The Stage position anchor (surface origin vs window-geometry origin) is an undecided B2/B3 question. Any client with CSD shadows (`geometry.loc ≠ 0`) is mis-hit.

**EV-P10** · Sub-pixel input is discarded at two points: each pan delta is rounded (`input.rs:178-179`) and the pointer is rounded before inverse projection (`state.rs:178-179`).
VERIFIED (static) · REPO · HIGH · n/a ·
Per-event rounding of pan deltas means |delta| < 0.5 px pans nothing (high-polling-rate mice with small deltas, B11). Rounding points must be defined in B1/B2.

**EV-P11** · Destroyed toplevels are never removed from the Stage.
VERIFIED (static) · REPO · No `toplevel_destroyed` in `XdgShellHandler` impl (`handlers.rs:93-134`). `Stage::remove_element_by_surface` (`spatial.rs:166`) has no callers (`prototype-grep-inventory.txt`) · HIGH · n/a ·
Violates I10 and the C12 regression "client destruction leaves no stale Stage relationship". Lifecycle (B5) has no prototype basis.

**EV-P12** · Popups are unsupported. `new_popup` is a no-op, there is no `PopupManager`, and popups are never configured or rendered.
VERIFIED (static) · REPO · `handlers.rs:118` · HIGH · n/a ·
B6 has no prototype basis. Real applications' menus would not appear.

**EV-P13** · No `wl_surface.enter`/`leave` is ever sent.
VERIFIED (static) · REPO · No calls to `output_enter`, `Output::enter` or `SpaceElement` methods (`prototype-grep-inventory.txt`) · HIGH · n/a ·
Clients never learn their output, so preferred scale and transform logic is absent. B8/B9 have no prototype basis.

**EV-P14** · Frame callbacks go to every Stage element on every redraw regardless of visibility, and the redraw loop re-arms itself unconditionally.
VERIFIED (static) · REPO + SMITHAY · `winit.rs:93-100` (`throttle = Some(Duration::ZERO)`, primary output closure always `Some(output)`), `winit.rs:106` · HIGH · n/a ·
Off-screen clients run unthrottled at host refresh, and idle CPU is never zero. This is the **opposite** of the Hyprland-era behavior (§3.3) and of the product requirement (§3.1).

**EV-P15** · Relative-pointer and pointer-constraints globals are advertised but never serviced.
VERIFIED (static) · REPO · `relative_motion` is never called. `relative_motion_events_sent` is never incremented. `new_constraint` is a no-op (`handlers.rs:159-168`) · HIGH · n/a ·
"Relative pointer / pointer lock work" is **unsupported** by the prototype. Game input semantics (B10/B11) have no basis.

**EV-P16** · `ElementId` is a monotonic `u64` (never reused in-process, so no ABA). It has no generation, lookup by surface is a linear scan, and `StageElement` owns a `smithay::desktop::Window`.
VERIFIED FACT · REPO · `spatial.rs:12-13, 119-124, 150-164, 216-224` · HIGH · n/a ·
The "pure domain" is Smithay-coupled. Monotonic-never-reused IDs are a legitimate candidate for B4 alongside generational slots.

**EV-P17** · Winit assumptions leak into input and output: absolute motion hard-codes 1280×720 (`input.rs:50`), and the render loop lives inside the winit event closure (`winit.rs:50-113`). The output has `Transform::Flipped180` (`winit.rs:42`), which is advertised to clients via `wl_output`.
VERIFIED FACT · REPO · HIGH · n/a ·
Backend boundary (B12) must be designed fresh.

**EV-P18** · xwayland-satellite 0.8.3 started under the prototype, connected to Xwayland on `:25`, and an X11 kitty mapped as a Stage toplevel.
VERIFIED FACT · PROTO-MEASURE · `task-316.log` lines "Connected to Xwayland on :25", "registered new toplevel window ElementId(0)" after `DISPLAY=:25 … kitty` · MEDIUM-HIGH (the log does not name the client per element) · n/a ·
Satellite warned "Couldn't bind fractional scale manager" and "Could not bind xdg activation". It *uses* those protocols when offered, so they interact with the C6 XWayland boundary. Spawn path is hard-coded to `/home/vysaely/.local/bin`, display `:25` is fixed, and there is no supervision (`xwayland.rs:10-22`).

**EV-P19** · Hardware GLES works nested: GLES 3.2, Mesa 26.2.3, AMD RX 9070 XT (radeonsi gfx1201), EGL Wayland platform.
VERIFIED FACT · PROTO-MEASURE · `task-280.log` · HIGH · n/a · —

**EV-P20** · A nested Wayland client maps: two kitty toplevels registered.
VERIFIED FACT · PROTO-MEASURE · `task-280.log` · HIGH for mapping, NONE for correct rendering position or input · n/a · —

**EV-P21** · `smithay::desktop::Space` is not used.
VERIFIED FACT · REPO · grep · HIGH · — · Confirms Space is optional. See EV-S04 for what must then be done by hand.

**EV-P22** · Error handling: 17 `unwrap`/`expect` sites, including `insert_client(...).unwrap()` on client connect (`state.rs:146`), and 1 `unsafe` (calloop `Generic::get_mut` for `dispatch_clients`, `state.rs:154`, same pattern as Smithay examples). The verify suite runs with `assert!` on every startup (`main.rs:28`).
VERIFIED FACT · REPO · HIGH · — · Failure model (C8) must be designed fresh.

**EV-P23** · Test architecture: 2 unit tests (projection symmetry, pan invariance) plus a startup self-check. No protocol, headless or client tests.
VERIFIED FACT · REPO + PROTO-MEASURE (`cargo test`: 2 passed) · HIGH · — · C11 starts from zero.

### 5.2 Hyprland Infynitae (EV-H)

**EV-H01** · Pan is O(N): every pan motion event calls `moveWindowTo` for every mapped canvas window.
VERIFIED FACT · REPO · `infynitae.cpp:4556-4614` · HIGH · — · This is the accidental complexity the native design exists to remove (I2).

**EV-H02** · One camera, bound to one "canvas monitor" chosen by config or focus. Multi-monitor is detected, not modeled.
VERIFIED FACT · REPO · `infynitae.cpp:176-177, 304-` · HIGH · — · There is no prior product experience with multi-output cameras. B7/B8 are new ground.

**EV-H03** · Runtime identity is the stringified Hyprland window pointer.
VERIFIED FACT · REPO · `infynitae.cpp:107-108, 4866-4870` · HIGH · — · Anti-pattern named by I7.

**EV-H04** · Canvas is `f64`, accepted range ±1e9 (`isSafeCoordinate`), rounded to `int` at the compositor boundary (`safeDoubleToInt`).
VERIFIED FACT · REPO · `infynitae.cpp:92-105` · HIGH · — · The only historical data point for Canvas range. ±1e9 in f64 keeps sub-µpx precision (ulp(1e9) ≈ 1.2e-7), but that is arithmetic, not a decision.

**EV-H05** · Product requirements listed in §3.1.
PROJECT REQUIREMENT · REPO (README, ARCHITECTURE) · HIGH · User revises product intent · Inputs to B/C. Most are Level D for Foundation 01.

**EV-H06** · In Hyprland, off-screen canvas windows got no frame callbacks and stayed idle, and that was the desired product behavior.
ESTABLISHED (Hyprland) + PROJECT REQUIREMENT · REPO (`offscreen-thumbnails.md` §1, §2.5, §4) · MEDIUM (historical note, Hyprland-specific) · — · Conflicts with the master prompt's anti-starvation rule. See EV-W02, Q-01.

**EV-H07** · Absolute vs relative pointer games need different treatment. Clamping to viewport edges triggers game edge-pan. Focus dimensions are orthogonal.
ESTABLISHED (Hyprland) · REPO (`pointer-input-findings.md`) · MEDIUM · — · Policy input for B10/B11 only. Not evidence about Smithay.

### 5.3 Smithay 0.7.0 (EV-S)

**EV-S01** · Pin = smithay 0.7.0 (crates.io), the newest stable release as of 2026-10-04, published 2025-06-24.
VERIFIED FACT · SMITHAY · `smithay-pin.txt` · HIGH · A newer crates.io release · Level B pinning decision (Q-07).

**EV-S02** · The feature names `backend_winit`, `backend_egl`, `renderer_gl`, `wayland_frontend`, `desktop`, `backend_drm`, `backend_gbm`, `backend_libinput`, `backend_udev`, `backend_session_libseat`, `xwayland` exist in 0.7.0.
VERIFIED FACT · SMITHAY · `Cargo.toml [features]` · HIGH · — · —

**EV-S03** · `desktop::Window` holds no position. Position lives in `Space` when Space is used. `Window` is usable standalone (`geometry`, `bbox`, `bbox_with_popups`, `send_frame`, `take_presentation_feedback`, `on_commit`, `surface_under`, `user_data`).
VERIFIED FACT · SMITHAY · `desktop/wayland/window.rs` pub API list · HIGH · — · Stage can be the sole placement authority (I1) without fighting Smithay.

**EV-S04** · Outside `Space`, `wl_surface.enter`/`leave` must be driven by the compositor. `output_update` lives in `pub(crate) mod wayland` and is **not public** in 0.7.0. The public paths are `SpaceElement::output_enter(output, overlap)` / `output_leave(output)` on `Window` (the trait is re-exported), or per-surface `Output::enter`/`leave`. Space passes `overlap` **relative to the element's `bbox` origin** (`space/mod.rs:347-352`).
VERIFIED FACT · SMITHAY · `desktop/space/mod.rs:29`, `space/wayland/window.rs:38-80`, `wayland/output/mod.rs:285-320` · HIGH · — ·
Infynitae must compute per-output overlap itself from Camera projection. The mechanism exists (B8 Smithay layer).

**EV-S05** · `send_frames_surface_tree(surface, output, time, throttle, primary_output_fn)` sends callbacks if the surface's primary scan-out output equals `output`, **or** if the frame is overdue. `throttle = None` means never for invisible surfaces. `Some(d)` means at most every `d`. `Some(ZERO)` means always.
VERIFIED FACT · SMITHAY · `desktop/wayland/utils.rs:210-272, 484-505` (doc comment and code agree) · HIGH · — ·
**Smithay already provides a throttled off-screen fallback.** Anti-starvation can be a policy parameter, not new machinery (B9).

**EV-S06** · `default_primary_scanout_output_compare` prefers a new output only if the element's visible area there is ≥ 2× the current one, otherwise the output with the higher refresh rate.
VERIFIED FACT · SMITHAY · `backend/renderer/element/mod.rs:264-` · HIGH · — · A worked B8 input: for W on A (165 Hz) and B (144 Hz), primary output depends on visible-area ratio, then refresh. Policy can be replaced.

**EV-S07** · Preferred buffer scale/transform (`send_surface_state`) and fractional preferred scale (`FractionalScaleState::set_preferred_scale`) are **one value per surface**, sent only on change. Nothing in `desktop` or `wayland::output` sets them automatically.
VERIFIED FACT (absence by grep: MEDIUM-HIGH) · SMITHAY · `wayland/compositor/mod.rs:409-430`, `fractional_scale/mod.rs:225-262` · HIGH for the API, MEDIUM-HIGH for absence · A Smithay call site that sets preferred scale on output enter ·
For a window on 1.0 and 1.5 outputs simultaneously, **Infynitae policy must choose one scale**. Pan can change it (EV-W04).

**EV-S08** · Smithay's pointer has one location in one "global compositor space" (`MotionEvent.location`, `input/pointer/mod.rs:934-941`). The focus origin must be in that same space.
VERIFIED FACT · SMITHAY · HIGH · — ·
With per-output cameras, "global compositor space" cannot be Canvas without leaking camera math into Smithay's pointer, and cannot be a single output's logical space. **Which space it is, is a B2 decision** (likely an output-layout space).

**EV-S09** · `PointerGrab` trait plus `PointerHandle::{set_grab, unset_grab, is_grabbed, motion, relative_motion}` are public.
VERIFIED FACT · SMITHAY · `input/pointer/grab.rs:37`, `input/pointer/mod.rs:168-252` · HIGH · — · CameraPan and WindowMove can be Smithay grabs. Whether that is the right ownership is B10.

**EV-S10** · Protocol state modules exist: `presentation` (`PresentationState::new(dh, clk_id)`), `fractional_scale`, `xdg_activation`, `pointer_constraints`, `relative_pointer`, `viewporter`.
VERIFIED FACT · SMITHAY · constructors located · HIGH · — · Feature-level availability only. Behavior is not verified.

**EV-S11** · Smithay's xdg layer tracks the `Suspended` toplevel state.
VERIFIED FACT (existence only) · SMITHAY · `wayland/shell/xdg/mod.rs:955-956` · MEDIUM · — · See EV-W05.

**EV-S12** · Possible Smithay convention mismatch: Space passes `overlap` relative to `bbox.loc`, but `Window::refresh` → `output_update` traverses from surface origin (0,0). When `bbox.loc ≠ (0,0)`, enter/leave may be computed against a shifted rectangle.
OPEN HYPOTHESIS · SMITHAY · `space/mod.rs:347-349` vs `space/wayland/window.rs:68-80`, `space/wayland/mod.rs:24-80` · LOW-MEDIUM (not traced end-to-end) · Trace showing the offsets cancel · If real, Infynitae must not copy Space's convention blindly when calling `output_enter`.

### 5.4 Wayland protocol (EV-W)

Verbatim text: [`evidence/phase-a/wayland-protocol-excerpts.txt`](evidence/phase-a/wayland-protocol-excerpts.txt).

**EV-W01** · `xdg_toplevel.configure` carries only size and states. A toplevel has no client-visible position.
VERIFIED FACT · SPEC (xdg-shell `xdg_toplevel.configure`) · HIGH · — ·
Pan-without-configure is **protocol-possible**. I3 is a policy the protocol permits, not one it guarantees.

**EV-W02** · `wl_surface.frame`: "A server **should avoid** signaling the frame callbacks if the surface is not visible in any way, e.g. the surface is off-screen, or completely obscured." It also says "The server must send the notifications so that a client will not send excessive updates, while still allowing the highest possible update rate for clients that wait for the reply before drawing again."
VERIFIED FACT · SPEC (wayland.xml) · HIGH · — ·
**Off-screen callback starvation is spec-recommended behavior, not a protocol violation.** The master prompt's B9 rule ("MUST NOT be permanently deadlocked") is a **PROJECT REQUIREMENT stricter than the protocol**. Both Hyprland-style starvation and a throttled fallback (EV-S05) are protocol-legal. Choosing between them, per client class, is a Level A policy question (Q-01).

**EV-W03** · `wl_surface.enter` "is emitted whenever a surface's creation, movement, or resizing results in some part of it being within the scanout region of an output". A surface may overlap zero or more outputs.
VERIFIED FACT · SPEC · HIGH · — ·
**Camera pan is client-observable.** It must produce `enter`/`leave` as windows cross output viewports, and that can cascade to preferred-scale events (EV-W04). "Pan isolates clients" can only mean no configure and no input. It cannot mean no events at all. I3 and the C12 regressions must be worded accordingly.

**EV-W04** · `wl_surface.preferred_buffer_scale` and `wp_fractional_scale_v1.preferred_scale` are one value per surface, "sent whenever the compositor's preference changes". `set_buffer_scale` text: "pick the same buffer scale as the scale of the output that the surface is displayed on".
VERIFIED FACT · SPEC · HIGH · — ·
With W visible at 1.0 and 1.5 at once, the compositor must pick one, and the other output resamples. Pan moving W between outputs may change the preference and make the client re-render. Not a configure, but client-visible work (B8).

**EV-W05** · `xdg_toplevel` state `suspended` (since v6): "not ordinarily being repainted; for example because its content is occluded… or its outputs are switched off". State changes are delivered by `xdg_toplevel.configure`.
VERIFIED FACT · SPEC · HIGH · — ·
**Direct tension with I3.** If Infynitae marks off-screen windows `suspended`, a pan that moves a window off every camera **generates a configure**. Either `suspended` must not be driven by camera visibility, or I3 must be restated as "no translation-induced *geometry* configure". Level A, Q-02.

**EV-W06** · `xdg_toplevel.configure_bounds` (since v4): bounds "can for example correspond to the size of a monitor", and a change is followed by `configure`.
VERIFIED FACT · SPEC · HIGH · — · Bounds must not be derived from "the output the camera currently shows the window on", or pan across outputs would configure (B7/B8).

**EV-W07** · `xdg_popup.configure` position is "relative to the upper left corner of the window geometry of the parent surface". Constraint adjustment applies when the popup is "partially outside positioning boundaries **set by the compositor**".
VERIFIED FACT · SPEC · HIGH · — ·
Popups are parent-relative: no independent Canvas position (B6). The constraint boundary on an unbounded canvas viewed by several cameras is **pure Infynitae policy**, still open (Q-04).

**EV-W08** · `xdg_popup.grab`: nested grabbing popups, dismissal in reverse creation order, compositor may deny (immediate dismissal). Must be in response to a user-action serial.
VERIFIED FACT · SPEC · HIGH · — · B6/B10 PopupGrab must follow this. CameraPan starting during a popup grab needs a defined outcome.

**EV-W09** · `wp_presentation_feedback`: presentation is synchronized to one output (`sync_output`). `discarded` means "never displayed".
VERIFIED FACT · SPEC · HIGH · — · Off-screen commits must eventually resolve as `discarded`, not stay pending forever. For multi-output W, one output is the presentation clock.

**EV-W10** · `zwp_relative_pointer_v1.relative_motion` is "not coupled to wl_pointer.motion" and can be sent independently.
VERIFIED FACT · SPEC · HIGH · — · Pan input isolation must suppress relative motion separately from absolute motion (B11). The Hyprland audit found the same at the Hyprland level (ARCHITECTURE_OPTIONS.md, option B limits).

**EV-W11** · `lock_pointer`: activation is at the compositor's discretion ("The protocol provides no guarantee that the constraints are ever satisfied"). A surface can have pointer focus without an active lock.
VERIFIED FACT · SPEC · HIGH · — · The compositor may legally deactivate or defer locks around CameraPan (B11 policy freedom).

**EV-W12** · `xdg_activation_v1.activate`: "It's up to the compositor to display this information as desired… might decide not to follow through".
VERIFIED FACT · SPEC · HIGH · — · There is no protocol obligation to move a Camera on activation (B10/C4). Teleport vs notify is policy.

**EV-W13** · Pointer focus is a single surface per seat, and `wl_pointer.motion` coordinates are surface-local.
VERIFIED FACT · SPEC · HIGH · — · When W is visible on two outputs, surface-local coordinates must be computed through the camera of the output the pointer is on. Consistency between cameras (same Canvas point → same surface-local point at zoom 1) is required for I9.

**EV-W14** · `xdg_toplevel.move`: server "may ignore move requests". If triggered, the surface loses device focus.
VERIFIED FACT · SPEC · HIGH · — · Client-requested moves can map onto compositor-owned WindowMove.

---

## 6. Major assumptions now explicitly labeled

| ID | Assumption | Why it is still an assumption |
|---|---|---|
| AS-1 | Smithay 0.7.0 is an acceptable base (vs git master). | Master was not inspected (Q-07). |
| AS-2 | xwayland-satellite remains the XWayland strategy. | Only one X11 client was observed mapping. No input, popup, DnD or game testing. |
| AS-3 | One Smithay pointer location space can be defined that is consistent with multiple independent cameras. | EV-S08: not designed or tested. |
| AS-4 | `send_frames_surface_tree` with a throttle gives acceptable anti-starvation behavior for real clients (Mesa EGL/Vulkan FIFO, browsers, games). | No client was tested off-screen. How Mesa WSI behaves with withheld callbacks was **not researched** in Phase A. |
| AS-5 | Phase A sources are the complete Hyprland-era record. | Only `docs/` and the core plugin were read. The shell (`shell/`) and test suite were not studied in depth (they are not foundation-relevant). |

---

## 7. Open questions surfaced for Phase B (not decided here)

| ID | Question | Level | Evidence |
|---|---|---|---|
| Q-01 | Off-screen frame-callback policy: starve (spec-recommended, Hyprland-era product behavior) vs throttled fallback (Smithay mechanism, master-prompt rule), possibly per client class. | A | EV-W02, EV-S05, EV-H06, EV-P14 |
| Q-02 | Is `xdg_toplevel.suspended` driven by camera visibility? If yes, I3 must be restated. | A | EV-W05 |
| Q-03 | Stage position anchor: surface origin vs window-geometry origin. | A | EV-P09, spec `set_window_geometry` |
| Q-04 | Popup constraint boundary on an unbounded multi-camera canvas. | A | EV-W07 |
| Q-05 | The "global compositor space" for Smithay's pointer under multiple cameras. | A | EV-S08, EV-W13 |
| Q-06 | Single preferred scale for a surface visible on outputs of different scale. | A | EV-W04, EV-S07, EV-S06 |
| Q-07 | Pin crates.io 0.7.0 or a Smithay git revision. Update cadence. | B | EV-S01 |
| Q-08 | Canvas numeric model. No prototype data (it was i32). Hyprland used f64 ±1e9. | A | EV-P01, EV-H04 |
| Q-09 | Restated I3 / "pan isolates clients": which client-visible events pan may legitimately cause (enter/leave, scale). | A | EV-W03, EV-W04 |

---

## 8. Required experiments (identified, **not run**)

| ID | Experiment | Resolves |
|---|---|---|
| X-1 | Client-side protocol trace (`WAYLAND_DEBUG=1` or a purpose-built test client) counting `xdg_toplevel.configure`, `wl_surface.enter/leave`, `preferred_*scale`, `wl_pointer.*` and `relative_motion` during a scripted pan. | EV-P04, Q-09, I3 |
| X-2 | Off-screen behavior of real clients under (a) no callbacks and (b) throttled callbacks: Mesa EGL swap-interval-1 client, Vulkan FIFO client, Firefox, mpv, one game. Measure CPU/wakeups and whether they recover on return. | Q-01, AS-4 |
| X-3 | Two nested outputs with scales 1.0 and 1.5, one window spanning both. Observe enter/leave, preferred scale and fractional scale. | Q-06, B8 |
| X-4 | Trace Smithay `output_enter` overlap convention with a window whose `bbox.loc ≠ 0` (CSD shadow, popup left of parent). | EV-S12 |

---

## 9. Phase A gate (A6)

| Item | Status | Basis |
|---|---|---|
| Current Infynitae inspected | **DONE** for foundation-relevant parts | §3. Shell runtime and test suite only listed (AS-5) |
| Prototype 01 inspected | **DONE**, all source plus recorded logs | §2, §5.1, ledger. Note: prototype is uncommitted; hashes recorded |
| Smithay revision explicitly pinned and verified | **DONE** for 0.7.0 APIs relied on. Pin *strategy* open (Q-07) | §4, EV-S01…S12 |
| Architecture-relevant protocol semantics investigated | **DONE** at specification level. No behavioral experiments | §5.4 |
| Evidence matrix written | **DONE** | this file |
| Major assumptions explicitly labeled | **DONE** | §6, §7 |

**Phase A gate: met.** Per master prompt A6/E1 and the turn-control rule, Phase B does not begin until
the user explicitly clears it.

---
---

## 10. Evidence catalogue (Phase D D7, living)

Canonical living home of the EVID namespace (FOUNDATION_ARCHITECTURE §P0). Every evidence ID used anywhere in the package resolves to exactly one row below.

The historical definition stays where it was written: EV in §5 above; EB in BLOCKERS; EC in ARCHITECTURE; ED in audit/README; ER in audit/1R; ER2 in audit/2R; EE (Phase E corrective, unsealed) in audit/E1R.

**Class** says what kind of support the evidence gives. **Tier/category** follows the tier-honesty rule (FOUNDATION_TEST_PLAN §1).
- No HARNESS, MODEL or spike entry is evidence about the production adapter, a GPU, toolkits, games or physical timing.
- No Phase A–D evidence exercises the production adapter.

Count: 122 entries (114 through Phase D; EE-1…EE-8 added by the unsealed E1R).

<!-- living:EVID -->
| ID | What (short) | Class | Tier / category | Historical definition / artifact |
|---|---|---|---|---|
| EV-P01 | Prototype Canvas/Camera/projection are `i32`, not `f64`. | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P02 | `--verify` "configure traffic during pan: 0" and "world mutations during pan: 0" are hard-coded literals, not measurements. | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P03 | No recorded prototype run exercised camera pan or printed final runtime stats. | VERIFIED FACT (for recorded runs) · PROTO-MEASURE | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P04 | The prototype's pan path cannot send `xdg_toplevel.configure`. | OPEN HYPOTHESIS, strongly supported by static analysis · REPO + SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P05 | Pan mutates no Stage element position. | VERIFIED (static) · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P06 | Camera mutation is O(1). | VERIFIED (static + trivial microbenchmark 8 ns) · REPO + PROTO-MEASURE | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P07 | Prototype has one Camera and one Output. "Multi-viewport" exists only as arithmetic on two `Camera` values. | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P08 | Clients receive wrong surface-local pointer coordinates. | VERIFIED (static) · REPO + SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P09 | Rendering and hit-testing use different element origins. | VERIFIED (static) · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P10 | Sub-pixel input is discarded at two points: each pan delta is rounded (`input.rs:178-179`) and the pointer is rounded before inverse projection (`sta… | VERIFIED (static) · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P11 | Destroyed toplevels are never removed from the Stage. | VERIFIED (static) · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P12 | Popups are unsupported. `new_popup` is a no-op, there is no `PopupManager`, and popups are never configured or rendered. | VERIFIED (static) · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P13 | No `wl_surface.enter`/`leave` is ever sent. | VERIFIED (static) · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P14 | Frame callbacks go to every Stage element on every redraw regardless of visibility, and the redraw loop re-arms itself unconditionally. | VERIFIED (static) · REPO + SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P15 | Relative-pointer and pointer-constraints globals are advertised but never serviced. | VERIFIED (static) · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P16 | `ElementId` is a monotonic `u64` (never reused in-process, so no ABA). It has no generation, lookup by surface is a linear scan, and `StageElement` o… | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P17 | Winit assumptions leak into input and output: absolute motion hard-codes 1280×720 (`input.rs:50`), and the render loop lives inside the winit event c… | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P18 | xwayland-satellite 0.8.3 started under the prototype, connected to Xwayland on `:25`, and an X11 kitty mapped as a Stage toplevel. | VERIFIED FACT · PROTO-MEASURE | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P19 | Hardware GLES works nested: GLES 3.2, Mesa 26.2.3, AMD RX 9070 XT (radeonsi gfx1201), EGL Wayland platform. | VERIFIED FACT · PROTO-MEASURE | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P20 | A nested Wayland client maps: two kitty toplevels registered. | VERIFIED FACT · PROTO-MEASURE | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P21 | `smithay::desktop::Space` is not used. | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P22 | Error handling: 17 `unwrap`/`expect` sites, including `insert_client(...).unwrap()` on client connect (`state.rs:146`), and 1 `unsafe` (calloop `Gene… | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-P23 | Test architecture: 2 unit tests (projection symmetry, pan invariance) plus a startup self-check. No protocol, headless or client tests. | VERIFIED FACT · REPO + PROTO-MEASURE (`cargo test`: 2 passed) | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-H01 | Pan is O(N): every pan motion event calls `moveWindowTo` for every mapped canvas window. | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-H02 | One camera, bound to one "canvas monitor" chosen by config or focus. Multi-monitor is detected, not modeled. | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-H03 | Runtime identity is the stringified Hyprland window pointer. | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-H04 | Canvas is `f64`, accepted range ±1e9 (`isSafeCoordinate`), rounded to `int` at the compositor boundary (`safeDoubleToInt`). | VERIFIED FACT · REPO | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-H05 | Product requirements listed in §3.1. | PROJECT REQUIREMENT · REPO (README, ARCHITECTURE) | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-H06 | In Hyprland, off-screen canvas windows got no frame callbacks and stayed idle, and that was the desired product behavior. | ESTABLISHED (Hyprland) + PROJECT REQUIREMENT · REPO (`offscreen-thumbnails.md` §1, §2.5, §4) | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-H07 | Absolute vs relative pointer games need different treatment. Clamping to viewport edges triggers game edge-pan. Focus dimensions are orthogonal. | ESTABLISHED (Hyprland) · REPO (`pointer-input-findings.md`) | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S01 | Pin = smithay 0.7.0 (crates.io), the newest stable release as of 2026-10-04, published 2025-06-24. | VERIFIED FACT · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S02 | The feature names `backend_winit`, `backend_egl`, `renderer_gl`, `wayland_frontend`, `desktop`, `backend_drm`, `backend_gbm`, `backend_libinput`, `ba… | VERIFIED FACT · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S03 | `desktop::Window` holds no position. Position lives in `Space` when Space is used. `Window` is usable standalone (`geometry`, `bbox`, `bbox_with_popu… | VERIFIED FACT · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S04 | Outside `Space`, `wl_surface.enter`/`leave` must be driven by the compositor. `output_update` lives in `pub(crate) mod wayland` and is not public in… | VERIFIED FACT · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S05 | `send_frames_surface_tree(surface, output, time, throttle, primary_output_fn)` sends callbacks if the surface's primary scan-out output equals `outpu… | VERIFIED FACT · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S06 | `default_primary_scanout_output_compare` prefers a new output only if the element's visible area there is ≥ 2× the current one, otherwise the output… | VERIFIED FACT · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S07 | Preferred buffer scale/transform (`send_surface_state`) and fractional preferred scale (`FractionalScaleState::set_preferred_scale`) are one value pe… | VERIFIED FACT (absence by grep: MEDIUM-HIGH) · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S08 | Smithay's pointer has one location in one "global compositor space" (`MotionEvent.location`, `input/pointer/mod.rs:934-941`). The focus origin must b… | VERIFIED FACT · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S09 | `PointerGrab` trait plus `PointerHandle::{set_grab, unset_grab, is_grabbed, motion, relative_motion}` are public. | VERIFIED FACT · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S10 | Protocol state modules exist: `presentation` (`PresentationState::new(dh, clk_id)`), `fractional_scale`, `xdg_activation`, `pointer_constraints`, `re… | VERIFIED FACT · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S11 | Smithay's xdg layer tracks the `Suspended` toplevel state. | VERIFIED FACT (existence only) · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-S12 | Possible Smithay convention mismatch: Space passes `overlap` relative to `bbox.loc`, but `Window::refresh` → `output_update` traverses from surface o… | OPEN HYPOTHESIS · SMITHAY | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W01 | `xdg_toplevel.configure` carries only size and states. A toplevel has no client-visible position. | VERIFIED FACT · SPEC (xdg-shell `xdg_toplevel.configure`) | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W02 | `wl_surface.frame`: "A server should avoid signaling the frame callbacks if the surface is not visible in any way, e.g. the surface is off-screen, or… | VERIFIED FACT · SPEC (wayland.xml) | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W03 | `wl_surface.enter` "is emitted whenever a surface's creation, movement, or resizing results in some part of it being within the scanout region of an… | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W04 | `wl_surface.preferred_buffer_scale` and `wp_fractional_scale_v1.preferred_scale` are one value per surface, "sent whenever the compositor's preferenc… | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W05 | `xdg_toplevel` state `suspended` (since v6): "not ordinarily being repainted; for example because its content is occluded… or its outputs are switche… | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W06 | `xdg_toplevel.configure_bounds` (since v4): bounds "can for example correspond to the size of a monitor", and a change is followed by `configure`. | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W07 | `xdg_popup.configure` position is "relative to the upper left corner of the window geometry of the parent surface". Constraint adjustment applies whe… | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W08 | `xdg_popup.grab`: nested grabbing popups, dismissal in reverse creation order, compositor may deny (immediate dismissal). Must be in response to a us… | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W09 | `wp_presentation_feedback`: presentation is synchronized to one output (`sync_output`). `discarded` means "never displayed". | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W10 | `zwp_relative_pointer_v1.relative_motion` is "not coupled to wl_pointer.motion" and can be sent independently. | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W11 | `lock_pointer`: activation is at the compositor's discretion ("The protocol provides no guarantee that the constraints are ever satisfied"). A surfac… | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W12 | `xdg_activation_v1.activate`: "It's up to the compositor to display this information as desired… might decide not to follow through". | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W13 | Pointer focus is a single surface per seat, and `wl_pointer.motion` coordinates are surface-local. | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EV-W14 | `xdg_toplevel.move`: server "may ignore move requests". If triggered, the surface loses device focus. | VERIFIED FACT · SPEC | Phase A (static/source unless EXPERIMENT) | this document §5 (Phase A, 2c8c163) |
| EB-1 | Numeric experiment (E1–E9): precision, accumulation, path dependence, jitter, adjacency, large-coordinate edges, round trip, equality | MATH/numeric experiment | offline | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: `experiments/b1-numeric/`, output `evidence/phase-b/b1-numeric-output.txt` |
| EB-2 | `Space` experiment (S1–S7) with a dummy `SpaceElement` against real Smithay 0.7.0 | HARNESS (Smithay Space) | T1-style harness | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: `experiments/b2-space/`, output `evidence/phase-b/b2-space-output.txt` |
| EB-3 | Smithay source: render-element rounding (`backend/renderer/element/surface.rs:93-139, 186-232, 263, 294`); `from_surface` takes `Point<f64, Physical>… | SMITHAY SOURCE | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source |
| EB-4 | Smithay source: `Space` internals (`desktop/space/mod.rs:38-56, 185-200, 258-365, 494-512`) | SMITHAY SOURCE | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source |
| EB-5 | Smithay/wayland-backend source: role destruction and client-kill paths (`shell/xdg/handlers/surface/toplevel.rs:156-188`, `popup.rs:69-95`; wayland-b… | SMITHAY SOURCE | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source |
| EB-6 | Smithay source: xdg unmap detection (`shell/xdg/mod.rs:1608-1640`), opt-in `ensure_configured` (`:1652-1673`), `Window::geometry` clamping (`desktop/… | SMITHAY SOURCE | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source |
| EB-7 | Protocol text: xdg_surface/xdg_toplevel/xdg_popup interface descriptions, `xdg_toplevel.destroy`, `xdg_popup.{destroy,popup_done,reposition}`, `xdg_t… | PROTOCOL | spec text | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: `/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml` (wayland-protocol… |
| EB-8 | Minimal lockstep harness. A Smithay 0.7.0 server (`wl_compositor` v6, xdg-shell, shm, fractional-scale, viewporter, two `wl_output` globals) plus a r… | HARNESS | T1-style harness | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: `experiments/b8-harness/` (`server.rs`, `client.rs`, `lockstep.rs`, `bin/scenar… |
| EB-9 | Real clients (vkcube/radv, kitty/swrast, Firefox 157) against the harness in real time on a private socket, no host compositor. They were shown on sc… | REAL-CLIENT | T3-style (real time, private socket) | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: `experiments/b8-harness/src/bin/realclient.rs`; output `evidence/phase-b/x2-rea… |
| EB-10 | SMITHAY: `CompositorState::new` advertises `wl_compositor` v5. v6 (`preferred_buffer_scale`) needs `new_v6` (`wayland/compositor/mod.rs:680-706`). Pr… | SMITHAY SOURCE + HARNESS | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source + harness (v6 bind fails against `new`) |
| EB-11 | SMITHAY: pending frame callbacks accumulate across commits (`frame_callbacks.extend`, `wayland/compositor/handlers.rs:146`). A withheld callback is n… | SMITHAY SOURCE + HARNESS | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source; confirmed by SC3 |
| EB-12 | SMITHAY: `send_surface_state` sends an integer preferred scale; `FractionalScaleState::set_preferred_scale` sends `round(scale·120)`; both send only… | SMITHAY SOURCE + HARNESS | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source; confirmed by SC1 |
| EB-13 | X-8: the pan-ownership model under test (`experiments/b8-harness/src/input.rs`) driven by injected physical events. Real Smithay seat, `wp_relative_p… | HARNESS | T1-style harness | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: `src/bin/x8.rs`; `evidence/phase-b/x8-pan-input-output.txt` |
| EB-14 | X-6: Smithay's real `WaylandSurfaceRenderElement` geometry (via `renderer_test`'s `DummyRenderer`) for a root surface plus an odd-offset subsurface.… | HARNESS (DummyRenderer) | T1-style harness | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: `src/bin/x6.rs`; `evidence/phase-b/x6-edges-output.txt` |
| EB-15 | X-9: presentation feedback while unobserved. A superseded commit's feedback is `discarded` at supersede time and never presented; the latest unobserv… | HARNESS | T1-style harness | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: `src/bin/x9.rs`; `evidence/phase-b/x9-presentation-output.txt` |
| EB-16 | SMITHAY: `pointer_constraints` `activate()` is never automatic (compositor policy); `deactivate()` destroys oneshot constraints (`wayland/pointer_con… | SMITHAY SOURCE + HARNESS | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source + X-8 |
| EB-17 | SMITHAY: `PresentationFeedbackCachedState::merge_into` discards superseded feedback; `Drop` discards on destruction (`wayland/presentation/mod.rs:315… | SMITHAY SOURCE | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source + X-9 |
| EB-18 | SMITHAY: `RendererSurfaceState::surface_view` is private; the public accessor is `view()` (`backend/renderer/utils/wayland.rs:344`). `SubsurfaceCache… | SMITHAY SOURCE | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source + X-6 (compile error, then `view()`) |
| EB-19 | SMITHAY: `xdg_wm_base` advertised at v6 (`shell/xdg/mod.rs:1252`). `XdgActivationHandler::token_created` default accepts every token (`xdg_activation… | SMITHAY SOURCE | source | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: source |
| EB-20 | PROTOCOL + EXPERIMENT: `zwp_relative_pointer_v1` deltas are `wl_fixed` (1/256 px). An unaccelerated 0.9 arrives as 0.898 (X-8). The protocol is coars… | PROTOCOL + HARNESS | T1-style harness | BLOCKERS §0 / B8.1 / B10.1 (4a0807e); artifact: X-8 |
| EB-H1 | Window::refresh popup membership offset by geometry.loc − popup.geometry.loc (confirmed by X5) | HARNESS | T1-style harness | BLOCKERS B2.4 / B8.5 (4a0807e) |
| EC-1 | Authority spike: two-crate workspace (domain stand-in with zero dependencies, protocol-adapter stand-in). 3 behaviour tests; 4 negative build probes,… | COMPILER PROBE (spike) | T0 spike | ARCHITECTURE evidence tables (Phase C); artifact: `experiments/c1-authority-spike/` (`verify.sh`); `evidence/phase-c/c1-authority… |
| EC-2 | Stable rustdoc does not check `compile_fail` error codes (a doctest marked `compile_fail,E0999` passed). `compile_fail` doctests alone cannot prove *… | TOOLING FACT | local toolchain | ARCHITECTURE evidence tables (Phase C); artifact: spike (transient edit, reverted) |
| EC-3 | SMITHAY: one XKB state per seat, updated for every key including intercepted ones; `pressed_keys` vs `forwarded_pressed_keys`; `set_focus` sends `ent… | SMITHAY SOURCE | source | ARCHITECTURE evidence tables (Phase C); artifact: `input/keyboard/mod.rs:209-297, 940-1050, 1315-1365` |
| EC-4 | PROTOCOL (wayland 1.26): `wl_keyboard.leave` "resets all values to their defaults"; `enter` sets the keys logically down; `key`: "must not send … rel… | PROTOCOL | spec text | ARCHITECTURE evidence tables (Phase C); artifact: `wayland.xml` |
| EC-5 | SMITHAY: `OutputDamageTracker::render_output` takes elements front-to-back; `render_elements_from_surface_tree` traverses `with_surface_tree_downward… | SMITHAY SOURCE | source | ARCHITECTURE evidence tables (Phase C); artifact: `backend/renderer/damage/mod.rs:313,461`; `element/surface.rs:107`; `renderer/m… |
| EC-6 | X-10 keyboard/modifier ownership on Smithay 0.7.0's real seat keyboard, with a client-side `wl_keyboard` protocol checker. 9 scenarios PASS; negative… | HARNESS (+ negative control) | T1-style harness | ARCHITECTURE evidence tables (Phase C); artifact: `experiments/b8-harness/src/{keys.rs,bin/x10.rs}`; `evidence/phase-c/x10-keyboa… |
| EC-7 | wayland-backend 0.3.17 does not send an event whose object argument is a destroyed object (`get_object(..)?` → `InvalidId`, `rs/server_impl/client.rs… | SOURCE (wayland-backend) | source | ARCHITECTURE evidence tables (Phase C); artifact: source + X-10 K-6 |
| EC-8 | Surface-tree snapshot via public API only (external crate compiles): root, subsurfaces (incl. negative-offset, placed *below*), input regions, window… | HARNESS | T1-style harness | ARCHITECTURE evidence tables (Phase C); artifact: `src/snapshot.rs`, `bin/xsnap.rs`; `evidence/phase-c/xsnap-x15-output.txt` |
| EC-9 | X-15 PASS: snapshot front-to-back order and positions equal Smithay's `Window::render_elements` exactly | HARNESS | T1-style harness | ARCHITECTURE evidence tables (Phase C); artifact: same |
| EC-10 | W-SM-11 (new): Smithay tests input regions with `point.to_i32_round()` (round-to-nearest) but surface bounds with exact containment, shifting every i… | SMITHAY SOURCE + HARNESS | source | ARCHITECTURE evidence tables (Phase C); artifact: source + xsnap (1,448 grid mismatches, all explained) |
| EC-11 | W-SM-12 (new): `under_from_surface_tree` overwrites its result for every match and early-stops only after a node's whole subtree (`desktop/wayland/ut… | SMITHAY SOURCE + HARNESS | source | ARCHITECTURE evidence tables (Phase C); artifact: source + xsnap |
| EC-12 | C6: xwayland-satellite 0.8.3 against the harness: X11 window → ordinary `xdg_toplevel` (title from X11, `app_id` = WM_CLASS); an X11 move request (`x… | REAL-CLIENT (xwayland-satellite) | T3-style | ARCHITECTURE evidence tables (Phase C); artifact: `realclient` + `evidence/phase-c/c6-sat.sh`; `evidence/phase-c/c6-xwayland-sate… |
| EC-13 | W-SM-13 (new): Smithay's *default* `XdgShellHandler::{maximize_request, fullscreen_request}` call `send_configure()` (`shell/xdg/mod.rs:1143-1153`).… | SMITHAY SOURCE + REAL-CLIENT | T3-style | ARCHITECTURE evidence tables (Phase C); artifact: source + C6 run (with and without override) |
| EC-14 | PROTOCOL: `set_fullscreen` / `unset_fullscreen` / state `fullscreen` text (subject to compositor policy; centre + border fill; "other screen content… | PROTOCOL | spec text | ARCHITECTURE evidence tables (Phase C); artifact: `xdg-shell.xml` |
| EC-15 | Authority-spike hardening: ports are `!Send`; protocol state is statically asserted `Send`. Storing a port in protocol state fails with E0277. All pr… | COMPILER PROBE (spike) + HARNESS | T0 spike | ARCHITECTURE evidence tables (Phase C); artifact: `experiments/c1-authority-spike/verify.sh`; `b8-harness/src/bin/sendcheck.rs`;… |
| EC-16 | Device limits (vulkaninfo, RX 9070 XT / RADV): `maxImageDimension2D = 32768`, `maxFramebufferWidth = 16384`. A client cannot render a buffer wider th… | DEVICE FACT (vulkaninfo) | local hardware query | ARCHITECTURE evidence tables (Phase C); artifact: `vulkaninfo`; `fractional-scale-v1.xml` |
| EC-17 | scaleprobe: GTK 4.22.5, kitty 0.49.2 (swrast), Firefox 157. All three honour a 1/120 change with a full re-render and a new buffer (628→633, 1920→193… | REAL-CLIENT | T3-style | ARCHITECTURE evidence tables (Phase C); artifact: `b8-harness/src/bin/scaleprobe.rs`; `evidence/phase-c/cp3a-scaleprobe-output.tx… |
| EC-18 | Buffer-cost table (computed) for candidate ceilings, double/triple buffering, plus a compositor copy for shm | MATH (computed) | model | ARCHITECTURE evidence tables (Phase C); artifact: `evidence/phase-c/cp3a-buffer-cost.txt` |
| EC-19 | Executable MODEL of the unified scale policy (not an experiment): walkthroughs A–E with the candidate constants | MODEL | model | ARCHITECTURE evidence tables (Phase C); artifact: `experiments/cp3a-scale-model/model.py`; `evidence/phase-c/cp3a-scale-model-out… |
| EC-20 | xhit: hit-testing at k ∈ {1.0, 1.25, 1.5, 1.75} × 4 fractional camera positions, every pixel over a tree with a nested subsurface, a below-parent sub… | HARNESS (DummyRenderer) | T1-style harness | ARCHITECTURE evidence tables (Phase C); artifact: `bin/xhit.rs`; `evidence/phase-c/cp3a-xhit-output.txt` |
| EC-21 | RC-1 with a real toolkit (GTK 4.22.5): the client destroys its keyboard-focused window while Shift+A are held. Observed: no `leave` (undeliverable, E… | REAL-CLIENT (GTK4) | T3-style | ARCHITECTURE evidence tables (Phase C); artifact: `bin/rc1.rs`, `cp3a-rc1-gtk4.py`; `evidence/phase-c/cp3a-rc1-gtk4-output.txt` |
| EC-22 | Feedback MODEL (not an experiment): client-geometry/scale loops under viewport-fit, five client behaviours; boundary wiggle. Benign clients converge… | MODEL | model | ARCHITECTURE evidence tables (Phase C); artifact: `experiments/cp3a-scale-model/feedback.py`; `evidence/phase-c/cp3b-feedback-mod… |
| EC-23 | Target hardware for budgets: AMD Ryzen 5 7600X3D, 30 GiB RAM, AMD Radeon RX 9070 XT (RADV GFX1201), Mesa 26.2.3, Linux 7.2.8 | DEVICE FACT | local hardware query | ARCHITECTURE evidence tables (Phase C); artifact: `/proc/cpuinfo`, `free`, vulkaninfo, EB-9 |
| ED-1 | d5hit: the D5 tree (root + nested subsurface + below-subsurface reached through an input-region hole + popup + popup subsurface) under the exact D5 c… | HARNESS (DummyRenderer) | T1-style harness | audit/README (e8ec03a) |
| ED-2 | d5_model: executable MODEL of the sealed rules. It computes the four D5 epochs and checks F1–F8 (lattice at zoom ≠ 1, SC-D/discrete-pan churn, device… | MODEL | model | audit/README (e8ec03a) |
| ED-3 | SMITHAY source: `PopupKeyboardGrab::set_focus` ignores focus changes while the grab is active; Smithay restores focus to the root on grab end (`deskt… | SMITHAY SOURCE | source | audit/README (e8ec03a) |
| ED-4 | SMITHAY source: `OutputDamageTracker::from_output` takes its scale from the `Output`; `OutputDamageTracker::new(size, scale, transform)` accepts any… | SMITHAY SOURCE | source | audit/README (e8ec03a) |
| ED-5 | HARNESS source: the harness's initial preferred scale is the maximum scale of outputs whose viewport contains the anchor point, ignoring zoom (`b8-ha… | HARNESS SOURCE | source | audit/README (e8ec03a) |
| ER-1 | xpopup_focus: Smithay `grab_popup` + `PopupKeyboardGrab`/`PopupPointerGrab`; core changes ActiveElementFocus while a popup grab is active. 10 scenari… | HARNESS (+ negative controls) | T1-style harness | audit/1R/CORRECTIONS_1R (84ce620) |
| ER-2 | xlock_grab: active lock + client implicit grab + interaction-output loss (D5 epoch 4 at protocol level). | HARNESS (+ negative controls) | T1-style harness | audit/1R/CORRECTIONS_1R (84ce620) |
| ER-3 | xmaximize: maximize and fullscreen answer policy. | HARNESS (+ negative controls) | T1-style harness | audit/1R/CORRECTIONS_1R (84ce620) |
| ER-4 | Authority spike, 1R. `Authorized` proof minted only in the port-carrying entry module; one-shot `FitViewport`. Probe 2d: protocol re-fit fails with E… | COMPILER PROBE (spike) | T0 spike | audit/1R/CORRECTIONS_1R (84ce620) |
| ER-5 | d5_model_1r: R1 pan algebra; R2 ScaleInteractionEpoch with negative control and timeout sensitivity; R3 one-shot fit; R4 D5 deltas; R5 confinement wi… | MODEL (+ negative controls) | model | audit/1R/CORRECTIONS_1R (84ce620) |
| ER-6 | xwayland-satellite 0.8.3 `--test-listenfd-support` exits 0 (the binary documents "return 0 if supports -listenfd"). Run with no display variables and… | LOCAL BINARY (dependency capability) | local | audit/1R/CORRECTIONS_1R (84ce620) |
| ER-7 | Regression re-run of d5hit, xhit, xsnap, x10, x8, x12, x6, x9 and scenarios after the additive harness changes; d5_model.py re-run | HARNESS regression | T1-style harness | audit/1R/CORRECTIONS_1R (84ce620) |
| ER2-1 | Authority spike 2R: OutputLifecycleToken probes p6–p9, protocol camera creation, token in protocol state, 2g single-construction check; 8 unit tests | COMPILER PROBE (spike) + seeded negative control | T0 spike | audit/2R/INVARIANT_DIFF_2R (5827926); evidence/phase-d/authority-spike-2r-verify.txt |
| ER2-2 | d5_model_2r: ScaleInteractionEpoch for all spatial interactions; window drag 39 → 1 with negative control | MODEL (+ negative control) | model | audit/2R/INVARIANT_DIFF_2R (5827926); evidence/phase-d/d5-model-2r-output.txt |
| EE-1 | xtransform: rotated outputs through Smithay 0.7.0 `OutputDamageTracker::render_output` into a recording renderer with GLES2 viewport/scissor semantics; 10 transform × scale cases; sealed tracker size and both readings of the sealed hit rule as negative controls | HARNESS (+ negative controls) | headless harness; no GPU, no T4 claim | evidence/E1R/xtransform-output.txt; experiments/b8-harness/src/bin/xtransform.rs |
| EE-2 | xlock_batch: 11 multi-batch constraint sequences on the wire, published Smithay (2/11 valid) vs Smithay + P1 (11/11 valid) | HARNESS (+ negative control) | T1-style harness | evidence/E1R/xlock-batch-{stock,p1}-output.txt; experiments/e1r-smithay-p1 |
| EE-3 | xzero: zero-output transition, 6 cases (none, persistent, oneshot, confinement, held button, held button + lock) + N1 sealed F-RELOCATE-1 on ∅, N2 no grab handling, N3 `unset_grab` under a lock; identical on published Smithay and P1 | HARNESS (+ negative controls) | T1-style harness | evidence/E1R/xzero-{stock,p1}-output.txt |
| EE-4 | Authority spike E1R: `output_reconfigured`, `PointerState`/`NoOutput`, relocation domain, OutputKey collisions; 14 tests; probes p10–p13; verify.sh 2h (with seeded negative control) and 2i | COMPILER PROBE (spike) + tests + seeded negative controls | T0-style spike | evidence/E1R/spike-verify-output.txt |
| EE-5 | e1r_hmax: F-SCALE-2 domain at zero extent (0×0, 0×N, N×0, unbuffered trees, first buffer with P7) and the integer-emission device bound; sealed formula raises / exceeds `D_max` | MODEL (+ negative controls) | model | evidence/E1R/e1r-hmax-output.txt |
| EE-6 | e1r_fullscreen: canonical scale policy for a fullscreen element on a 1.0 output observed from a 2.0 output at zoom 1–8; h ≤ H_cfg; the 8K claim does not follow | MODEL | model | evidence/E1R/e1r-fullscreen-output.txt |
| EE-7 | P1 regression: x8 (48/48 unchanged), x12 (W-SM-4b no longer reproduced), xlock_grab (EXPLICIT/persistent becomes valid; SEALED still fails) rebuilt against Smithay + P1 | HARNESS regression | T1-style harness | evidence/E1R/p1-regress-*.txt |
| EE-8 | E1R regression re-run of the affected historical experiments and models on the published Smithay | HARNESS / MODEL regression | as each original | audit/E1R/REGRESSION_E1R.md |
<!-- /living -->
