# Layer surfaces (panels, bars, launchers, wallpapers) — decisions needed (2026-10-21)

Labels: [F] verified · [I] interpretation · [R] recommendation · [O] owner decision.
**Status: PROPOSAL.** Nothing here is decided or implemented.

**Why:**
- The owner chose a Quickshell-class shell (OWNER_DECISIONS_2026-10-21 #4).
- Quickshell draws bars, overlays and backgrounds through `zwlr_layer_shell_v1` [F, Quickshell build docs].
- Infynitae offers no layer-shell [F, flaw scan F2], and docs/foundation never mentions layer surfaces, so this is missing architecture.

**Available [F]:** Smithay 0.7 (vendored, pinned) has `wayland::shell::wlr_layer`: `Layer`, `Anchor`, `ExclusiveZone`, `Margins` and `KeyboardInteractivity`.

The Canvas rules stay intact under every option below. A layer surface is not an element:
- it has no anchor, no Stage entry and no camera effect;
- INV-CAMERA-1/2 and the Stage are unaffected.

## Decisions

| # | Question | Options | [R] |
|---|---|---|---|
| L1 | What is a layer surface in Core? | (a) a new object, `LayerSurface` (OBJ-35): per output, screen-fixed in output-logical coordinates, outside the Canvas, never moved by a camera, never in membership's camera views · (b) an element pinned to a camera (would move with the Canvas math) · (c) adapter-only, unknown to Core | **(a)**: Core must know them for hit testing, focus and the usable area |
| L2 | Drawing order | `background` and `bottom` below the Canvas elements, `top` and `overlay` above them, per output (the protocol's meaning) | **as the protocol says** |
| L3 | Hit testing | one ordered list per output: overlay → top → Canvas (ProjectionFrame) → bottom → background, input regions respected; the ProjectionFrame stays the Canvas part's record (F-HIT-1 unchanged) | **that order** |
| L4 | Exclusive zones (a bar reserving its strip) | (a) they shrink the output's **usable area**, which F-PLACE-1 (new windows) and F-FIT-1 (fit) use; the camera's view, membership and scale are unchanged · (b) they shrink the camera's view itself (moves the Canvas mapping; affects every invariant built on View) · (c) ignored | **(a)**: placement and fit avoid the bar, and nothing else changes |
| L5 | Keyboard focus | `none`: never focused · `on_demand`: focused by a click on it, like a window (INV-INPUT-8 explicit action) · `exclusive`: takes the keyboard while mapped in `top`/`overlay` (a launcher), and gives it back to the previous element when it goes | **as the protocol says**, with the focus history restored |
| L6 | Fullscreen presentations (C5) | `overlay` layers stay above a presentation; `top`, `bottom` and `background` are hidden while an output presents (the usual compositor behaviour, so a fullscreen video covers the bar) | **that** |
| L7 | Pointer constraints and camera pan over a layer | a pan (Logo+drag) that starts on a layer surface still pans (the binding wins, as over windows); constraints apply to layer surfaces like to element surfaces | **that** |
| L8 | Who may create layer surfaces | (a) any client (the protocol's default; Quickshell needs it) · (b) only the shell the compositor launched | **(a)** for now, like the shell socket's same-user trust (C7.1) |

## What this would take [I]

- **New layer and tests:** a new amendment layer (C12.F19) covering OBJ-35, the hit-test order, the usable area in F-PLACE-1/F-FIT-1, and focus rules in M12; T1 tests with a layer-shell probe client; seeded controls.
- **Adapter:** Smithay's `WlrLayerShellState` handlers, mapped into Core through protocol entries (as toplevels are).
- **Renderer:** draws the four layers around the Canvas.
- **Live check:** Quickshell inside a nested Infynitae (needs your per-run authorization).

Reply "go with the recommendation", or name the rows to change.
