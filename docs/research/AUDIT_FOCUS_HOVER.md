# Audit: FOCUS_FOLLOWS_MOUSE and game hover (`AUDIT_FOCUS_HOVER`)

Status (Claude's review, 2026-11-15): fixed in fix/hover-focus-held-button: finding 1 (no hover focus while an app holds a mouse button; the crossing counts after the release) and finding 4 (constraints reconciled after a hover focus). Finding 5 fixed in fix/hover-after-camera-move (hover_seen resynced after every camera write and at the end of an interaction). Finding 7 fixed in fix/strict-successor-keeps-panel. Findings 2, 3, 6, 8 and improvement 10 fixed in fix/focus-audit-rest. Open: improvements 9, 11, 12 (reproducers ignored).

- **Scope**: `FOCUS_FOLLOWS_MOUSE` (`off` / `sloppy` / `strict`), `hover_seen`, `hovered_window`, `strict_successor`, `settle_hover`, pointer constraint reconciliation/validity, `Layer` interactivity/occlusion, camera pans and glides, and stepped-aside/windowed game focus oscillation (`app_id = "space.bigrat.mocktail"`).
- **Files audited**: `crates/infynitae-core/src/entry.rs`, `crates/infynitae-core/src/input/focus.rs`, `crates/infynitae-core/src/lib.rs`, `crates/infynitae-core/src/layer.rs`, `crates/infynitae-core/src/protocol.rs`, `crates/infynitae-wayland/src/handlers.rs`, `crates/infynitae-wayland/src/outputs.rs`, `vendor/smithay-0.7.0/src/wayland/pointer_constraints.rs`.
- **Not checked**: live DRM/libinput session and real desktop compositor runtime (prohibited by audit constraints), `xwayland-satellite` C/X11 pointer-warp translation, and Roblox/Sober client internals (external binary). XWayland wire handling in `crates/infynitae-wayland/src/xwayland.rs` was checked and contains only X11 socket/supervisor management.

---

## Ranked findings

| # | Kind | Finding | Evidence | How sure | What the user would see | Fix | Size |
|---|---|---|---|---|---|---|---|
| 1 | Bug | Hover focus steals keyboard focus while the client holds a mouse button | `crates/infynitae-core/src/entry.rs:1989-2014`, `crates/infynitae-core/src/entry.rs:6643-6680`, `crates/infynitae-backend-headless/tests/audit_focus_hover.rs:97`, `242-251` | Proved by Core + wire tests | Holding RMB to orbit camera in a stepped-aside or windowed game flips `wl_keyboard` and `activated` state rapidly between `GAME` and another window | Gate `hover_focus` in `Interaction::Idle` on `!self.input.ownership.client_owns_any()` (or update `hover_seen` when buttons release) | S |
| 2 | Bug | `Layer::Overlay` / `Top` surfaces leave `hover_seen` stale and trap `OnDemand` keyboard focus | `crates/infynitae-core/src/entry.rs:1989-2014`, `crates/infynitae-core/src/entry.rs:6738`, `6818`, `crates/infynitae-backend-headless/tests/audit_focus_hover.rs:255-318` | Proved by Core + wire tests | Clicking an `OnDemand` edge panel over a window and moving back into the same window leaves keyboard focus trapped on the panel | Update `self.hover_seen = None` when `layer_hit(..., true).is_some()` and handle `from_layer` when re-entering `under == self.hover_seen` | S |
| 3 | Bug | `route_pointer_hit_local` and active `Lock` validity ignore `Layer::Overlay` / `Top` occlusion | `crates/infynitae-core/src/protocol.rs:2851-2865`, `2902-2932`, `2962-2978`, `crates/infynitae-core/src/entry.rs:6827`, `6909` | Proved by Core test | A pointer lock activates behind an `Overlay` (such as a mana/HUD overlay or panel) covering the pointer, and relative motion goes to the overlay | Check `self.layer_hit(output, p, true).is_none()` in `route_pointer_hit_local` and `Lock` validity in `is_constraint_still_valid` | S |
| 4 | Bug | `hover_focus` omits `reconcile_constraints()` | `crates/infynitae-core/src/entry.rs:1989-2014`, `crates/infynitae-core/src/entry.rs:6684`, `6727` | Proved by Core test | Hover-focusing a game window that already has a pending `zwp_pointer_constraints_v1` request leaves the lock/confine inactive until another input reconciles | Call `out.extend(self.reconcile_constraints())` when `hover_focus` changes `focus.target()`, matching `set_focus` | S |
| 5 | Bug | Camera pans and zoom glides leave `hover_seen` stale without pointer motion | `crates/infynitae-core/src/entry.rs:1876-1917`, `2037-2096`, `2685-2705`, `6917`, `6963`, `6972`, `7036` | Proved by Core tests (glide + pan) | After `FocusNext`/glide or `CameraPan` puts a different window under the stationary cursor, a 1 px mouse nudge steals focus back | Resync `self.hover_seen = self.hovered_window()` after `camera_written`, `pan_step`, `end_interaction`, `MoveElement`, `touch_recenter`, and `focus_cluster_slot` | S |
| 6 | Bug | `strict_successor` runs after `target_lost`, emitting a double focus change and stripping successor `attention` | `crates/infynitae-core/src/lib.rs:740-752`, `crates/infynitae-core/src/protocol.rs:1622-1626`, `1733-1737`, `crates/infynitae-core/src/input/focus.rs:222-253`, `crates/infynitae-core/src/entry.rs:7045`, `7067` | Proved by Core test | In `strict` mode, closing a window over empty canvas flashes focus onto an urgent window, clears its `attention` mark, and drops focus to `None` | Resolve the strict hovered/None successor before or inside `target_lost` instead of mutating focus twice, and update `hover_seen` | M |
| 7 | Bug | `strict_successor` steals keyboard focus from an active `OnDemand` layer | `crates/infynitae-core/src/lib.rs:749-751`, `crates/infynitae-core/src/input/focus.rs:138-144`, `249-251`, `crates/infynitae-core/src/entry.rs:7079`, `7151` | Proved by Core test | Typing in an `OnDemand` edge panel while an underlying window closes or unmaps tears keyboard focus away from the panel | Preserve `self.focus.layer()` (or avoid calling `set_focus` over `from_layer`) in `strict_successor` when an `OnDemand` layer holds the keyboard | S |
| 8 | Bug | Non-keyboard focus switches drop `>= 300 ms` hover-focused windows from `Super+Tab` MRU history | `crates/infynitae-core/src/entry.rs:101-125`, `2830-2837`, `crates/infynitae-core/src/input/focus.rs:91-93`, `113-123`, `crates/infynitae-core/src/entry.rs:7160`, `7205-7221` | Proved by Core test | Using a game/window hovered for > 300 ms, then clicking or switching via `ShellCommand::Focus` instead of pressing a key, drops that window from `Super+Tab` | Call `self.focus.settle_hover(self.input.clock.now_us())` (or timestamped `set_focus`) before clearing `hovered_since` | S |
| 9 | Improvement | Game-protection rules needed to stop focus oscillation in stepped-aside / windowed games (`space.bigrat.mocktail`) | `crates/infynitae-core/src/entry.rs:165-170`, `1989-2014`, `crates/infynitae-core/src/protocol.rs:718` | Checked in code | Macros, third-person RMB drags, unlocked warps, or `Super` key taps over a stepped-aside/windowed game flip focus at full motion rate | Keep focus latched during client-held buttons, constrain/dwell/cooldown hysteresis, and optional game-session sticky hover | M |
| 10 | Improvement | `client_minimize` calls `target_lost` + `strict_successor` without `reconcile_constraints()` | `crates/infynitae-core/src/protocol.rs:2315-2336` | Checked in code | If minimizing a focused window exposes/unfocuses a window with a pointer constraint, constraints wait until the next motion event to reconcile | Append `self.reconcile_constraints()` and update `hover_seen` in `client_minimize` | S |
| 11 | Improvement | `PanMode::Free` and `PanMode::Rts` move `self.pointer` and emit `true_position_into` during pan without FFM / `hover_seen` alignment | `crates/infynitae-core/src/entry.rs:2063-2095` | Checked in code | During Free/RTS pan the cursor visibly moves onto new windows (`wl_pointer.enter`) while keyboard focus freezes, then nudging afterwards behaves inconsistently | Sync `hover_seen` in `pan_step` / `end_interaction`, and clarify whether `Free`/`Rts` pan intentionally defers hover focus until release | S |
| 12 | Improvement | `Super` releases active pointer locks on stepped-aside or windowed games (`!in_fullscreen`) | `crates/infynitae-core/src/entry.rs:165-170` | Checked in code | Pressing `Super` once in a windowed/stepped-aside game unlocks the cursor immediately so the next macro sweep triggers hover focus switches | Optionally require double-tap, hold delay, or game-lock toggle when a constraint is active | S |

---

## When hover moves focus

Infynitae implements `FOCUS_FOLLOWS_MOUSE` via two booleans in `Core` (`crates/infynitae-core/src/entry.rs:1989-2014`):
- **Off** (`focus_follows_mouse == false`): hover never moves keyboard focus.
- **Sloppy** (`focus_follows_mouse == true && focus_follows_mouse_strict == false`): crossing (`under != self.hover_seen`) focuses `Some(under)` when `under != target`, and leaves focus unchanged when `under.is_none()`.
- **Strict** (`focus_follows_mouse == true && focus_follows_mouse_strict == true`): crossing onto a window focuses it; crossing onto `None` (empty canvas or gap) clears keyboard focus (`hover_focus(None, time_us)`) whenever `target.is_some()`.

Exact behavior per situation in the current tree:

| Situation | Observed behavior (`FOCUS_FOLLOWS_MOUSE` enabled) | Evidence |
|---|---|---|
| Crossing between two windows | Updates `hover_seen = Some(new)`; if `new != focus.target()`, immediately calls `focus.hover_focus(Some(new), time_us)`. Fires on 1 motion event even if a client mouse button is held; never calls `reconcile_constraints()`. | `entry.rs:1989-2014` |
| Crossing into a gap / empty canvas | Updates `hover_seen = None`. **Sloppy**: keeps current focus. **Strict**: calls `hover_focus(None, time_us)` (`KeyboardFocus(None)`). Returning into the **same** window sees `Some(w) != None`, so it re-triggers `hover_focus` (re-focusing in sloppy too if another window focused while in the gap). | `entry.rs:2002-2012` |
| Moving onto a `Top` or `Overlay` layer (`above_canvas()`) | `layer_hit(cur_o, cur_p, true)` is `Some(_)`, so lines `1989-2014` are skipped completely: keyboard focus does not move, and **`hover_seen` is not updated** (stays `Some(W)`). Clicking an `OnDemand` layer focuses the layer (`focus.layer = Some((layer, false))`); moving back onto `W` sees `under == hover_seen == Some(W)`, so `W` is not re-focused and the panel keeps keyboard focus. | `entry.rs:1994`, `2666-2672`, `layer.rs:373-384` |
| Moving onto a `Bottom` or `Background` layer (`!above_canvas()`) | `layer_hit(..., true)` is `None`. Hovering a `Bottom`/`Background` layer exposed in a gap evaluates `under = None`. In **Strict**, moving off a window onto an exposed `Bottom` layer clears `focus.target()` **and** clears any `OnDemand` layer keyboard holder (`from_layer` in `focus_to`, `focus.rs:138-144`). Clicking it (`OnDemand`) takes keyboard focus until a different window (`!= hover_seen`) is entered. | `entry.rs:1994-2012`, `focus.rs:138-144`, `2656-2672` |
| Active fullscreen presentation (`!is_presentation_suppressed`) | Only the presented window is in `RenderList`; `layer_frame` hides non-`Exclusive` `Top` layers and all `Bottom`/`Background` layers (`layer.rs:345-366`), and `KeyboardKey` sets `in_fullscreen = true` so `Super` cannot suspend pointer constraints (`entry.rs:165-170`). Hover over the single fullscreen window never changes `under`. | `layer.rs:345-366`, `entry.rs:165-170`, `5724-5741` |
| Active `ConstraintKind::Lock` | `Interaction::Idle` calls `locked_motion()` and **returns early** at `entry.rs:1964-1968` before `pointer::moved` and `focus_follows_mouse`. However, `route_pointer_hit_local` (`protocol.rs:2851-2865`) and `Lock` validity (`protocol.rs:2915-2932`) ignore `above_canvas` layers: if an `Overlay` sits over the pointer, `Lock` can still activate and `locked_motion` (`protocol.rs:2968`) routes `Effect::PointerRelative` to the `Overlay`. Destroying a lock (`handlers.rs:434` / `smithay` `pointer_constraints.rs`) removes the constraint without calling `hover_focus` until the next physical motion. | `entry.rs:1964-1968`, `protocol.rs:2851-2978`, `entry.rs:6827`, `6909` |
| Active `ConstraintKind::Confine` | `Interaction::Idle` calls `confined_motion()` + `routed_motion()` and **returns early** at `entry.rs:1969-1973` before `focus_follows_mouse`. Physical cursor stays clamped to the surface region, so crossing off the window cannot happen unless `Super` suspends constraints (`entry.rs:168-170`) or an `Overlay` layer covers the confined region (`route_pointer_hit_local` invalidates only when another surface — never a layer — wins `hit_test`). | `entry.rs:1969-1973`, `protocol.rs:2851-2865`, `2933-2943` |
| During `CameraPan` (or `ElementMove`/`Resize`) | `pointer_motion` matches `Interaction::CameraPan` (`entry.rs:1938-1945`), calls `pan_step()`, and skips lines `1960-2016` completely: hover never moves focus during a pan. `end_interaction()` (`entry.rs:2685-2705`) reconciles constraints and emits `true_position()` without updating `hover_seen`. | `entry.rs:1938-1959`, `2685-2705` |
| After camera move without pointer motion | Immediate camera writes in `FocusNext` (`entry.rs:762`), `Recenter` (`entry.rs:801`), and `jump_to_window` (`entry.rs:1289`) snapshot `self.hover_seen = self.hovered_window()` synchronously **before** any animation tick. Asynchronous `zoom_tick` glide frames (`camera_written`, `entry.rs:1876-1917`), `pan_step` / `end_interaction`, `focus_cluster_slot` (`1307-1342`), `ShellCommand::MoveElement` (`2845-2870`), `layer_pointer` (`layer.rs:651-659`), and `touch_recenter` (`1170-1188`) emit `true_position_into` (`wl_pointer.enter`) without touching `hover_seen`. Keyboard focus does not move on the camera move itself, but `hover_seen` is now stale, so a subsequent 1 px nudge inside the *same* window triggers `under != hover_seen` and steals focus. | `entry.rs:762`, `801`, `1289`, `1307-1342`, `1876-1917`, `2685-2705`, `2845-2870` |
| Focused window closes / unmaps / minimizes | `target_lost` runs MRU fallback (`history`), then `strict_successor` overrides focus to `hovered_window().filter(\|&e\| e != gone)` in `strict` mode without updating `hover_seen` (`lib.rs:740-752`). | `protocol.rs:1622-1626`, `1733-1737`, `2330-2335`, `lib.rs:740-752` |

---

## Bugs

### 1. Hover focus steals keyboard focus while a client holds a pointer button (and causes Roblox `GAME`/other flipping)

- **Evidence**: `crates/infynitae-core/src/entry.rs:1989-2014`, `crates/infynitae-core/src/entry.rs:2444-2459`, `crates/infynitae-core/src/entry.rs:6643-6680`, `crates/infynitae-backend-headless/tests/audit_focus_hover.rs:97`, `242-251`.
- **Why it happens**: Compositor drags (`button_press` at `entry.rs:2452,2458`) explicitly check `!self.input.ownership.client_owns_any()`, and active compositor drags keep `self.input.interaction` out of `Interaction::Idle`. A client-owned mouse press stays in `Interaction::Idle` and only records ownership in `self.input.ownership`. Line `1989` checks `if self.focus_follows_mouse` without checking `self.input.ownership.client_owns_any()`, so any motion crossing a window edge (or gap in `strict`) while a client holds a button calls `hover_focus` and sends `wl_keyboard.leave` / `ToplevelConfigure(activated=false)` while `wl_seat` still considers the client to own the pressed button drag.
- **Why this explains the `space.bigrat.mocktail` macro log**:
  1. In active fullscreen (`!is_presentation_suppressed`), the game covers the entire output, non-`Exclusive` top panels are hidden (`layer.rs:345-366`), and `Super` cannot suspend pointer constraints (`entry.rs:165-170`), so focus stays on `GAME`.
  2. Once fullscreen is stepped aside (`is_presentation_suppressed == true`, `entry.rs:5724-5741`) or the game is windowed, sibling windows and gaps become exposed under the pointer. Any input phase where no Wayland `Lock`/`Confine` is active — third-person RMB camera orbit with button held, unlocked cursor warps (XWayland/Sober warp internals not checked), `Confine` released or crossed via `Super` suspension (`entry.rs:168-170` permits suspension because `!in_fullscreen`), or between lock toggles — lets every `PointerMotion` cross between `GAME` and `other` at full motion/macro rate with zero button guard and zero dwell/cooldown. (Re-sent `set_fullscreen` from the game when focus bounces back is already kept aside by `protocol.rs:718`.)
- **Reproducers (observed failing)**:
  - `entry::tests::audit_hover_focus_steals_keyboard_focus_while_client_button_held` (`entry.rs:6675`): `left: Some(ElementId(2)), right: Some(ElementId(1))` while `client_owns_any() == true`.
  - `roblox_stepped_aside_flips_keyboard_focus_between_game_and_other_while_button_held` (`audit_focus_hover.rs:242`): verbatim wire log during two horizontal sweeps with `BTN_RIGHT` held on stepped-aside `space.bigrat.mocktail`:
    `game_kb=["toplevel_configure GAME activated=false", "wl_keyboard.leave GAME", "toplevel_configure GAME activated=true", "wl_keyboard.enter GAME keys={}", ...]`, `other_kb=["toplevel_configure other activated=true", "wl_keyboard.enter other keys={}", ..., "toplevel_configure other activated=false", "wl_keyboard.leave other", ...]`.

### 2. Above-canvas `Top` / `Overlay` layer surfaces leave `hover_seen` stale and trap `OnDemand` keyboard focus

- **Evidence**: `crates/infynitae-core/src/entry.rs:1989-2014`, `crates/infynitae-core/src/entry.rs:2666-2672`, `crates/infynitae-core/src/input/focus.rs:138-144`, `crates/infynitae-core/src/entry.rs:6738`, `6818`, `crates/infynitae-backend-headless/tests/audit_focus_hover.rs:255-318`.
- **Why it happens**: At `entry.rs:1994`, `&& self.layer_hit(cur_o, cur_p, true).is_none()` bypasses the entire `hover_seen` update when the pointer moves onto a `Top` or `Overlay` layer (such as an edge panel). If the pointer was over window `W` (`hover_seen == Some(W)`), moves onto an `OnDemand` panel and clicks it (`focus.layer = Some((panel, false))`, `focus.target() == Some(W)`), and then moves back onto `W`, `entry.rs:2003` compares `under == self.hover_seen` (`Some(W) == Some(W)`), skips `hover_focus`, and leaves keyboard focus stuck on the `OnDemand` panel even as the user moves and types back over `W`.
- **Reproducers (observed failing)**:
  - `entry::tests::audit_layer_hover_leaves_hover_seen_stale_and_traps_on_demand_layer_focus` (`entry.rs:6818`): `left: (Some(LayerId(1)), Some(ElementId(1))), right: (None, Some(ElementId(1)))`.
  - `top_layer_on_demand_panel_traps_keyboard_on_reentry_wire` (`audit_focus_hover.rs:312`): `left: Some("panel"), right: Some("W")`.

### 3. `route_pointer_hit_local` and `Lock` validity ignore `Layer::Overlay` / `Top` occlusion

- **Evidence**: `crates/infynitae-core/src/protocol.rs:2851-2865`, `2902-2932`, `2962-2978`, `crates/infynitae-core/src/lib.rs:2231-2276`, `crates/infynitae-core/src/entry.rs:6827`, `6909`.
- **Why it happens**: `route_pointer` (`lib.rs:2231-2276`) checks `layer_hit(o, p, true)` before windows. By contrast, `route_pointer_hit_local` (`protocol.rs:2851-2865`) calls `self.hit_test(output, pt)` directly without checking `self.layer_hit(output, p, true).is_none()`, and `is_constraint_still_valid` for `ConstraintKind::Lock` (`protocol.rs:2915-2932`) only checks that the projected lock point lies inside the output bounds `[0, extent)`. Thus an `Overlay` (for example a mana/HUD overlay with input region, or an edge panel over a window) does not stop `can_activate_constraint`, does not deactivate an existing `Lock` or `Confine`, and `locked_motion` (`protocol.rs:2968`) then sends `Effect::PointerRelative` to `route_pointer()`, delivering the game's locked relative deltas to the `Overlay`.
- **Reproducer (observed failing)**:
  - `entry::tests::audit_overlay_layer_over_window_still_activates_pointer_constraint` (`entry.rs:6909`): fails with `"an Overlay layer covering the pointer must block constraint activation on the window underneath"`.

### 4. `hover_focus` omits `reconcile_constraints()`

- **Evidence**: `crates/infynitae-core/src/entry.rs:1989-2014`, `crates/infynitae-core/src/lib.rs:957-959`, `crates/infynitae-core/src/entry.rs:6684`, `6727`.
- **Why it happens**: Explicit focus helpers (`Core::set_focus` at `lib.rs:958`, `show_opened` at `entry.rs:1718`, `raise_and_focus` at `entry.rs:1296`, `ShellCommand::Focus` at `entry.rs:2836`) call `self.reconcile_constraints()` right after changing focus. `Interaction::Idle` calls `self.focus.hover_focus(under, time_us)` at `entry.rs:2011` and never reconciles constraints, leaving a pending `zwp_locked_pointer_v1` / `zwp_confined_pointer_v1` on `under` inactive even though `under` now holds both pointer focus and keyboard focus.
- **Reproducer (observed failing)**:
  - `entry::tests::audit_hover_focus_does_not_activate_pending_pointer_constraint` (`entry.rs:6727`): fails with `"hover-focusing ws[1] should reconcile and activate its pending pointer constraint"`.

### 5. Camera pans and asynchronous zoom glides leave `hover_seen` stale without pointer motion

- **Evidence**: `crates/infynitae-core/src/entry.rs:762`, `801`, `1289`, `1307-1342`, `1876-1917`, `2037-2096`, `2685-2705`, `2845-2870`, `crates/infynitae-core/src/layer.rs:651-659`, `crates/infynitae-core/src/entry.rs:6917`, `6963`, `6972`, `7036`.
- **Why it happens**: `FocusNext` (`entry.rs:762`), `Recenter` (`801`), and `jump_to_window` (`1289`) snapshot `self.hover_seen = self.hovered_window()` synchronously right after queueing a smooth `glide_to` recenter, before the camera has moved. Subsequent `zoom_tick` / `camera_written` frames (`1876-1917`) slide another window under the stationary cursor and emit `true_position_into` (`wl_pointer.enter`) without updating `self.hover_seen`. Likewise, `pan_step` (`2037-2096`) and `end_interaction` (`2685-2705`) (as well as `focus_cluster_slot` `1307-1342`, `touch_recenter` `1170-1188`, `MoveElement` `2845-2870`, and `layer_pointer` `layer.rs:651-659`) change what lies under the cursor without syncing `hover_seen`. When the user later moves the mouse by 1 px within the new window, `under != hover_seen` is true and steals focus away from the intended window. (Checked in code: `FocusNext`/`Recenter`/`jump_to_window` only avoid this when `glide` happens to be instant; any multi-tick glide triggers it.)
- **Reproducers (observed failing)**:
  - `entry::tests::audit_zoom_glide_after_focus_next_leaves_hover_seen_stale_so_nudge_steals_focus` (`entry.rs:6963`): `left: Some(ElementId(3)), right: Some(ElementId(2))`.
  - `entry::tests::audit_camera_pan_leaves_hover_seen_stale_so_nudge_steals_focus` (`entry.rs:7036`): `left: Some(ElementId(2)), right: Some(ElementId(1))`.

### 6. `strict_successor` runs after `target_lost`, emitting transient focus and clearing successor `attention`

- **Evidence**: `crates/infynitae-core/src/lib.rs:740-752`, `crates/infynitae-core/src/protocol.rs:1622-1626`, `1733-1737`, `crates/infynitae-core/src/input/focus.rs:222-253`, `crates/infynitae-core/src/entry.rs:7045`, `7067`.
- **Why it happens**: On unmap (`protocol.rs:1623-1625`), destroy (`1734-1736`), and minimize (`2330-2331`), Core first runs `focus.target_lost(id, &live)`, which synchronously focuses the MRU history fallback (`ws[0]`), emits `Effect::ToplevelConfigure(ws[0])` + `Effect::KeyboardFocus(Some(ws[0]))`, and clears `ws[0]`'s `attention` mark (`focus.rs:240`). Immediately after, `strict_successor` calls `focus.set_focus(hovered)`, which in `strict` mode over empty canvas emits `Effect::KeyboardFocus(None)`. Even worse, `strict_successor` never updates `self.hover_seen = hovered`.
- **Reproducer (observed failing)**:
  - `entry::tests::audit_strict_successor_emits_double_focus_and_clears_attention` (`entry.rs:7067`): emits `[ToplevelConfigure(ElementId(2)), ToplevelConfigure(ElementId(1)), KeyboardFocus(Some(ElementId(1))), ToplevelConfigure(ElementId(1)), KeyboardFocus(None)]` and strips `ElementId(1)`'s attention mark.

### 7. `strict_successor` steals keyboard focus from an active `OnDemand` layer

- **Evidence**: `crates/infynitae-core/src/lib.rs:740-752`, `crates/infynitae-core/src/input/focus.rs:138-144`, `249-251`, `crates/infynitae-core/src/entry.rs:7079`, `7151`.
- **Why it happens**: `target_lost` (`focus.rs:249-251`) preserves an active layer (`if self.layer.is_none()`). Immediately afterward, `strict_successor` calls `self.focus.set_focus(hovered)` (`lib.rs:750`), and `focus_to` (`focus.rs:138-144`) unconditionally clears `from_layer = matches!(self.layer, Some((_, false)))` because `set_focus` models an explicit click/activation rather than passive window loss. Closing an unrelated target window underneath an `OnDemand` edge panel in `strict` mode therefore drops the panel's keyboard focus.
- **Reproducer (observed failing)**:
  - `entry::tests::audit_strict_successor_steals_on_demand_layer_focus` (`entry.rs:7151`): `left: None, right: Some(LayerId(1))`.

### 8. Non-keyboard focus switches omit `settle_hover`, dropping `>= 300 ms` windows from MRU history

- **Evidence**: `crates/infynitae-core/src/entry.rs:101-125`, `2830-2837`, `crates/infynitae-core/src/lib.rs:957`, `crates/infynitae-core/src/input/focus.rs:46-48`, `90-123`, `crates/infynitae-core/src/entry.rs:7160`, `7205-7221`.
- **Why it happens**: `settle_hover(now_us)` (`focus.rs:113-123`, threshold `HOVER_SETTLES_US = 300_000`) is only called from `KeyboardKey` (`entry.rs:161`) and `hover_focus` (`focus.rs:106`). Switching focus via `ShellCommand::Focus` (`entry.rs:2835`), `Core::set_focus` (`lib.rs:957`), `PointerButton` click (`entry.rs:101-125`, `2665`), or `show_opened` (`entry.rs:1717`) calls `focus.set_focus(...)`, which clears `self.hovered_since = None` (`focus.rs:92`) without first settling the previously hover-focused window. If the user dwelled in a window for `>= 300 ms` (reading, scrolling, or using the mouse) before switching via panel click or `ShellCommand::Focus`, `Super+Tab` skips that window.
- **Reproducer (observed failing)**:
  - `entry::tests::audit_hover_over_300ms_before_non_keyboard_focus_switch_is_dropped_from_history` (`entry.rs:7216`): `left: Some(ElementId(2)), right: Some(ElementId(3))`.

---

## Improvements

### 9. Proposed rules to keep a game focused while playing (`space.bigrat.mocktail`)

Existing tests establish two desktop constraints that rules must preserve:
- `focus_follows_mouse_hover_focuses_window` (`entry.rs:4433`, observed passing): plain pointer motion from one desktop window to another with no button pressed and no dwell option active moves focus immediately.
- `super_tab_keeps_its_window_when_the_mouse_is_nudged` (`entry.rs:5510`, observed passing): a 1 px nudge inside the currently hovered window does not re-focus it after `FocusNext`.

To stop the `GAME`/`other` flip in stepped-aside fullscreen games, windowed games, and macro sweeps without breaking desktop hover, apply five layered rules:

1. **Rule R1 — Never hover-focus while a client holds a pointer button**:
   - In `Interaction::Idle` (`entry.rs:1989`), skip `hover_focus` whenever `self.input.ownership.client_owns_any()` is `true` (whether `hover_seen` stays frozen or syncs on final `button_release` when `!client_owns_any()`).
   - Stops all third-person RMB camera drags and macro sweeps holding `BTN_LEFT`/`BTN_RIGHT` over exposed siblings or gaps.
   - *Case R1a*: Stepped-aside `space.bigrat.mocktail` holding `BTN_RIGHT` while sweeping cursor across sibling `other` and back keeps `wl_keyboard` on `GAME`.
   - *Case R1b*: Releasing all held buttons over `other` after the drag reconciles (`hover_seen` vs current window) cleanly according to dwell/cooldown rules.

2. **Rule R2 — Occlusion-strict constraints + post-constraint hover cooldown and sync**:
   - Keep `Lock` and `Confine` exclusive (`entry.rs:1964-1973`), fix `route_pointer_hit_local` + `Lock` validity (`protocol.rs:2851-2932`) so `Top`/`Overlay` surfaces block activation/relative forwarding (`locked_motion`), call `reconcile_constraints()` after `hover_focus`, and when a `Lock`/`Confine` deactivates or is destroyed (`handlers.rs:434` / `reconcile_constraints`), resync `hover_seen = hovered_window()` and start a short hover-suppress window (or require an explicit click / exit-and-reenter) before `hover_focus` can fire.
   - Prevents a single unlocked warp or constraint destroy from immediately throwing keyboard focus to an adjacent window on the next relative/absolute sample.
   - *Case R2a*: Active `Lock` on a windowed game + `Overlay` HUD/mana overlay: overlay receives no leaked `PointerRelative` events when it occludes `p_lock`, and unlocking onto the window requires leaving overlay / cooldown expiry before FFM switches away.
   - *Case R2b*: Pending `zwp_locked_pointer_v1` activates immediately when hovering into a game window (`reconcile_constraints` inside `hover_focus`), locking the cursor before the next macro sweep exits the window.

3. **Rule R3 — Crossing hysteresis / boundary dwell for rapid macro sweeps (and optional per-app game lock)**:
   - Track `(pending_hover_target, pending_hover_since_us)` on crossings: when crossing *away* from a window that owns an active/recent constraint, holds input, or matches a running game (`space.bigrat.mocktail` / active `presentation(o)` even while suppressed), require either `now_us - since >= HOVER_EXIT_DWELL_US` (or deliberate pointer stop) before switching keyboard focus to `other`/`None`, OR rate-limit hover focus transitions per `time_us` (cooldown/anti-flap hysteresis on rapid back-and-forth `A -> B -> A -> B` within a few hundred ms).
   - Note: keep dwell at `0` for normal desktop windows unless configured (preserving `focus_follows_mouse_hover_focuses_window`), enabling dwell / anti-oscillation whenever the currently focused window is a presented (including stepped-aside) game, holds a constraint, or has had `> 1` hover focus flip in the last `500 ms`.
   - *Case R3a*: Macro sweeps `GAME -> other -> GAME -> other` at 20 ms intervals with no button down and no lock: anti-oscillation cooldown holds focus on `GAME` instead of flipping every 20 ms.
   - *Case R3b*: Normal slow desktop crossing (`ws[0] -> ws[1]`) fires with zero delay when neither window is a game/constrained/oscillating.

4. **Rule R4 — Guard `free_cursor_on_super` on stepped-aside or constrained games**:
   - At `entry.rs:165-170`, tapping `Super` once currently drops `Lock`/`Confine` whenever `!in_fullscreen` (which includes stepped-aside fullscreen games and windowed games). Adding a brief hold threshold or explicit game-mode cursor-release guard (or keeping constraints suspended only *while* `Super` is down without resetting `hover_seen` until `Super` is paired with a drag/navigation binding) prevents accidental `Super` taps from freeing a game macro into FFM flips.
   - *Case R4a*: Stepped-aside fullscreen `space.bigrat.mocktail` with an active `Lock` receives a bare tap of `Super_L` (`press` + `release` with no motion/binding): `Lock` re-engages without triggering an intervening FFM focus switch.

5. **Rule R5 — Single `sync_hover_after_view_change()` helper for all camera/stack/layer/successor transitions**:
   - Whenever Core changes camera center/zoom (`camera_written`, `pan_step`, `end_interaction`, `touch_recenter`), moves/resizes elements (`MoveElement`), switches focus (`set_focus`, `focus_cluster_slot`, `ShellCommand::Focus`, `strict_successor`), or passes over `above_canvas` layers (`layer_hit(..., true)`), update `self.hover_seen` coherently with layer ownership (`OnDemand` vs cleared).
   - Ensures stationary pointer transitions never turn into spurious 1 px nudge focus steals inside a game or sibling window.
   - *Case R5a*: Pan or zoom-glide across `GAME` and `other`, then nudge pointer 1 px inside `other`: focus does not jump unintentionally; entering `W` after leaving a clicked `Top`/`OnDemand` panel clears `from_layer` and restores focus to `W`.

### 10. `client_minimize` omits `reconcile_constraints()`

- **Evidence**: `crates/infynitae-core/src/protocol.rs:2330-2335`.
- **Detail**: `client_minimize` calls `target_lost` + `strict_successor` + `membership_pass_element` + `visibility_pass`, unlike `Core::set_focus` (`lib.rs:958`) and `show_opened` (`entry.rs:1718`). Adding `reconcile_constraints()` ensures a newly exposed constrained window or a minimized locked window reconciles immediately instead of waiting for motion.

### 11. `PanMode::Free` and `PanMode::Rts` pan behavior with `hover_seen`

- **Evidence**: `crates/infynitae-core/src/entry.rs:2063-2095`.
- **Detail**: During `CameraPan` in `PanMode::Free` or `PanMode::Rts`, `pan_step` clamps and updates `self.pointer` on every delta and emits `true_position_into(out)` (`wl_pointer.enter`), while `pointer_motion` stays in `Interaction::CameraPan` and never runs FFM or updates `hover_seen`. Right after releasing the pan button, moving 1 px over the window where the cursor already sits can still see `hover_seen` stale from before the pan (`end_interaction` omits `hover_seen = hovered_window()`, covered by Bug #5).

---

## Verification

All checks ran after the final `cargo fmt --all` edit with `nice -n 19` on targeted test suites only (no workspace suite, no live compositor).

1. Default Core audit tests stay ignored (suite stays green):
   - Command: `nice -n 19 cargo test -p infynitae-core --lib entry::tests::audit_`
   - Result: `ok. 0 passed; 0 failed; 9 ignored; 0 measured; 348 filtered out`
2. Ignored Core reproducers fail at their exact bug assertions:
   - Command: `nice -n 19 cargo test -p infynitae-core --lib entry::tests::audit_ -- --include-ignored --nocapture`
   - Result: `FAILED. 0 passed; 9 failed; 0 ignored; 0 measured; 348 filtered out` (`CORE_EXIT=101`; all 9 `audit_*` tests failed at their final assertions).
3. Default headless wire audit tests stay ignored (suite stays green):
   - Command: `nice -n 19 cargo test -p infynitae-backend-headless --test audit_focus_hover`
   - Result: `ok. 1 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out` (`common::oracle::tests::rounding_helpers` passed; both wire bug reproducers ignored).
4. Ignored headless wire reproducers fail at their exact bug assertions:
   - Command: `nice -n 19 cargo test -p infynitae-backend-headless --test audit_focus_hover -- --include-ignored --nocapture`
   - Result: `FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out` (`WIRE_EXIT=101`; `roblox_stepped_aside_flips_keyboard_focus_between_game_and_other_while_button_held` failed at line `242` with `wl_keyboard.leave`/`enter` flips; `top_layer_on_demand_panel_traps_keyboard_on_reentry_wire` failed at line `312` with `left: Some("panel"), right: Some("W")`).
5. Existing hover / `Super+Tab` tests remain green:
   - Command: `nice -n 19 cargo test -p infynitae-core --lib focus_follows_mouse && nice -n 19 cargo test -p infynitae-core --lib super_tab`
   - Result: `focus_follows_mouse`: `ok. 2 passed; 0 failed; 0 ignored; 0 measured; 355 filtered out`; `super_tab`: `ok. 5 passed; 0 failed; 0 ignored; 0 measured; 352 filtered out`.
