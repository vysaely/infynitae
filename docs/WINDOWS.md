# Windows: where they go, how they fit, what changes them

A map of window placement and every window interaction, with the code that does it. Written
2026-10-31 for the owner's question about maximize, fit and screen sizes; the rules themselves are
in `docs/foundation/` (cited by ID) and `docs/DECISIONS.md`.

Coordinates: a window's **anchor** is the Canvas point of its window-geometry origin (whole px).
Each screen shows its camera's view of the Canvas (centre, zoom). The **usable area** is a
screen's logical size less the bars' exclusive zones (L4). The **room** is the first screen's
usable area less the 12 px gap on each side.

## A new window

| Step | Where | What |
| :--- | :--- | :--- |
| Initial configure | `protocol.rs` `committed` (AwaitingInitialCommit) | 0x0 (the app chooses), `configure_bounds` = the room (INV-PROTO-1), a scale hint before it (M15). |
| First map | `protocol.rs` `first_map_anchor` | A notification: bottom right of the usable view. A child (dialog) clearly smaller than its parent: centred over the parent. The successor of an app's just-closed window: its place. Else by `PLACEMENT_POLICY`: `cluster` (the default in the config; owner rule 2026-10-31, F-PLACE-1) centres it on the pointer, kept inside the usable view; if that spot is not free, the nearest free spot (12 px gap kept), one wholly on screen first, else the nearest on the Canvas. It never overlaps or moves another window. `center` centres it in the usable view. |
| Too large | `handlers.rs` `fit_opening_window`, `protocol.rs` `place_opening_at_size` | A window that maps larger than the room (Chromium/Electron at a saved size, X11 through xwayland-satellite) is offered the room and placed again for that size by the same free-space rule; nothing is pushed (until 2026-10-31 it took the middle of the view and pushed the others aside). A window the user opened brings the camera to it (TELEPORT_ON_OPEN). |
| Grew by itself | `protocol.rs` `self_resized` | An app that grows its window by itself pushes nothing: if the new size reaches no new window it stays; else it keeps its right, bottom or both edges, or its centre, the first that is free; else it moves to the nearest free spot (2026-10-31). |
| Grew to a size asked for | `protocol.rs` `push_grown` | A maximize or a Super+right drag pushes the windows the new rect newly reaches, left or right, keeping the gap, and the push goes on along the cluster: a pushed window pushes the windows it then reaches, in the same direction, each moving once (owner, 2026-10-31). |

Everything above uses the **first** screen: its camera, usable area and room, whichever screen
the pointer is on.

## Sizes the compositor chooses

| Action | Where | What |
| :--- | :--- | :--- |
| Super+Enter, `SUPER_ENTER_ACTION="fit"` (default) | `entry.rs` `fit_toggle` | Camera zoom to fit the window under the pointer; again: the camera before. The window is not resized. |
| Super+Enter, `"maximize"` | `entry.rs` `maximize_toggle` | Zoom reset to 1; the window moves to the usable view's top-left and is asked for the usable size; overlapping neighbours pushed aside; its place and size before are kept (`saved_floating`). Again: back to them. Remembered with its screen (`Core::maximized`). |
| Super+Shift+Enter, an app's `set_fullscreen` | `entry.rs` `toggle_fullscreen`, `protocol.rs` (M9) | Protocol fullscreen on the pointer's screen: the screen shows only it, at the screen's logical size rounded down (F-PRESENT-1). Nothing on the Canvas moves (owner, 2026-10-31). Leaving: the size before (`FullscreenState.saved`); at its first windowed frame the window keeps its place if it is free at the size it draws, else it moves to the nearest free spot (it came back larger, or a window took its place); it never pushes (`returned_from_fullscreen`). A Super+drag out of fullscreen places it where the user lets go. |
| Super+right drag | `protocol.rs` `begin_resize`/`resize_to`/`end_resize` | From the nearest corner; one configure outstanding at a time; the opposite corner stays. Super+Shift+right drag scales a cluster. |
| App's `set_maximized`/`set_minimized` | `protocol.rs` `toplevel_request` | Declined / ignored (INV-PROTO-1): the compositor's maximize is Super+Enter. |

**Held sizes** (INV-PROTO-1, amended 2026-10-31): a size the compositor asked of a window is
carried by every later configure (focus changes send one), until the app acknowledges it and
draws another size of its own, or unmaps (`Core::held_sizes`, `toplevel_size_offered`,
`toplevel_size_drawn`). Before, later configures said 0x0, and with SQUARE_CORNERS (every window
told it is tiled) GTK 3 apps went back to the size they opened at: the maximize was lost at the
next window switch.

## Moving and the camera

| Input | What |
| :--- | :--- |
| Super+left drag / Super+Shift+left drag | Move the window / its cluster; out of fullscreen at the window's size before it. It may be dropped over other windows: nothing is pushed or kept apart (left as it is for the owner to decide). |
| Super+right drag | Resize from the nearest corner; the new size pushes the windows it newly reaches aside, left or right. |
| Touch: three fingers | Move or resize the window under them (see the touch design notes). |
| Super+Tab | Focus the next window; with `FOCUS_CYCLE_NAVIGATE` centre the camera on it. |
| Super+1..9 / Super+Shift+1..9 | Focus window / cluster N (creation order), raise, centre the camera (zoom 1) / frame the cluster. |
| Click | Focus and raise the window under the pointer. |
| Super+Space / Super+Shift+Space | Centre the focused window in the usable area at zoom 1 / at the current zoom. |

A focus change sends a configure to the window losing and the one gaining focus (`activated`);
with held sizes it no longer changes their size.

## Screens

| Change | What |
| :--- | :--- |
| New mode, scale or rotation (`output_reconfigured`) | Camera kept (same centre and zoom); bars arranged again; a fullscreen window is asked for the new size; a maximized window is asked for the new usable size and keeps its centre on the Canvas (amended 2026-10-31). Any other window larger than the new room is offered the room and stays where it is (2026-10-31); the rest keep their size and place. The room (bounds) reaches each window with its next configure. |
| A bar comes, goes or changes its exclusive zone | The usable area changes: maximized windows on that screen fill the new one (2026-10-31). |
| A second screen | Its own camera (restored from the persisted table by its connector/EDID, else new). Placement and the room stay those of the first screen. |
| A screen unplugged | Its camera is kept for its return; fullscreen on it ends (the size before is offered); a window maximized on it fills the first remaining screen (2026-10-31). Windows keep their Canvas places. |

## After a restart

`infynitae restore` (`crates/infynitae/src/restore.rs`) saves every 15 s each window's app,
title, anchor, size, fullscreen flag and maximized state (its place and size before the maximize)
and each screen's camera centre. At login it launches each app, moves each new window of a saved
app to the next saved anchor of that app (unless a window open before covers it), asks it for its
saved size or maximizes it again on the first screen (`resize_element`, 2026-10-31), and puts the
camera centres back. Not restored: zoom, fullscreen state. In the VM device runs (before 2026-10-31), the start terminal shown "NOT wholly on screen"
was pushed aside by the test's own oversized XTerm, which took the middle of the view; it was not
a restore misplacement. Since the free-space rule the XTerm goes to free space and the terminal
stays.

## Tests

`crates/infynitae-backend-headless/tests/`: `t1_window_states` (held sizes; maximized and
fullscreen windows across 1920x1080, 1366x768, 2560x1440 at 1/1.5/2, 3840x2160 at 2, portrait,
and every change between them; unplug; Super+Shift+Enter moves nothing), `t1_protocol_bounds`
(room, oversize fit), `t1_fit_constraints`, `t1_fit_bindings`, `t1_window_slots`,
`t1_window_drag`, `t1_fullscreen`, `t1_transform`, `t1_outputs`. Free space (new windows near
the cursor, self-resize, back from fullscreen in every way): `t1_window_states` too.

## Open questions (not changed)

Placement and the room follow the first screen only (owner, 2026-10-31: kept). Neighbours pushed
aside by a maximize stay pushed after the restore. Session restore does not bring back zoom or
fullscreen.
