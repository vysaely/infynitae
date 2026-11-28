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
| Initial configure | `protocol/toplevels.rs` `committed` (AwaitingInitialCommit) | 0x0 (the app chooses), `configure_bounds` = the room (INV-PROTO-1), a scale hint before it (M15). |
| First map | `protocol/placement.rs` `first_map_anchor` | A notification: bottom right of the usable view. A child (dialog, or an app's second window that names a parent), whatever its size: centred over the parent, which it may cover, as it may its own app's other windows; never another app's: one that would goes to the nearest spot free of them (Discord's main window, a child of its small updater). The next window of an app whose window just closed, with no key press, button press or touch since (an app swapping its own windows): its place. Else by `PLACEMENT_POLICY`: `cluster` (the default in the config; owner rules 2026-10-31 and 2026-11-22, F-PLACE-1) puts it beside a cluster (windows within 300 px of each other) showing in the usable view of the pointer's screen: the focused window's cluster when that window shows, else the cluster nearest the pointer. It touches that cluster on its side nearest the pointer, in the nearest free spot (12 px gap kept), one wholly on screen first; a spot level with a neighbour's edges wins over one about as near. With no room on screen it goes beyond the view, still beside the cluster: a window you opened brings the camera to it, one an app opened asks for attention. The first window in a view (no window showing in it) opens in the middle of the usable view, wherever the pointer rests. The view is where the camera stops: a window opening during a camera glide was kept in the view of that moment and ended off screen (2026-11-22). With `CLUSTER_SORT` "app" (off by default) a window whose app already has a window in the first screen's view aims right of the newest such window (12 px gap) instead, then the nearest free spot. It never overlaps or moves another window: one rule after every path above (but notifications and the `center` policy) moves a spot that would cover another app's window to the nearest free one. `center` centres it in the usable view. |
| Starts fullscreen or as large as a screen | `protocol/toplevels.rs` `toplevel_request`, `protocol/placement.rs` `fullscreen_map_spot`; `launch.rs` `opened_by_user` | A game (Roblox through Mocktail, a Steam game through Proton or SDL) takes the screen it opens on (2026-11-19). One that asks for fullscreen before it shows, or a frame after (mapped last, nothing pressed or touched and the focus not moved since), is granted it and takes the focus; it keeps a place on the Canvas that covers no other window, where it shows when it steps aside; at its first windowed frame it is placed as any new window. One that opens exactly as large as a screen is not offered the room. Either counts as a window the user opened (FOCUS_ON_OPEN and TELEPORT_ON_OPEN "user"): it takes the focus and the camera goes to it, so it fills the screen from its free spot and moves no other window. Until then such a game, started by a launcher long before its window showed, opened unfocused, its request was declined, and it stayed a window wherever the free space was, over or beside other windows. |
| Too large | `handlers.rs` `fit_opening_window`, `protocol/placement.rs` `place_opening_at_size` | A window that maps larger than the room (Chromium/Electron at a saved size, X11 through xwayland-satellite) is offered the room and placed again for that size by the same free-space rule; nothing is pushed (until 2026-10-31 it took the middle of the view and pushed the others aside). A window the user opened brings the camera to it (TELEPORT_ON_OPEN). |
| Grew by itself | `protocol/placement.rs` `self_resized` | An app that grows its window by itself pushes nothing: if the new size reaches no new window it stays; else it keeps its right, bottom or both edges, or its centre, the first that is free; else it moves to the nearest free spot (2026-10-31). |
| Grew to a size asked for | `protocol/toplevels.rs` `committed` | A maximize, a resize of any kind (Super+right drag, an app's edge, three fingers, a scaled cluster, a cluster layout's cell) or a session restore's size moves only that window; no other window moves, the window may cover them (owner, 2026-11-06; until then a maximize or a resize pushed the row aside). A maximized window stays where the maximize put it. |

A new window, a child kept in the view and a window brought into view use the pointer's screen
(else the first): its camera and usable area. Everything else above uses the **first** screen: its
camera, usable area and room, whichever screen the pointer is on.

## Sizes the compositor chooses

| Action | Where | What |
| :--- | :--- | :--- |
| Super+Enter, `SUPER_ENTER_ACTION="fit"` (the default without the line; the shipped config sets `"maximize"`) | `entry/windows.rs` `fit_toggle` | Camera zoom to fit the window under the pointer; again: the camera before. The window is not resized. |
| Super+Enter, `"maximize"` (shipped) | `entry/windows.rs` `maximize_toggle` | Zoom reset to 1; the window is asked for the usable size and placed where it covers no other window: the view's top-left when that is free, else the nearest free spot on the canvas, out of view if need be, and Super+Space goes to it (owner, 2026-11-10); no other window moves; its place and size before are kept (`saved_floating`). Again: back to them; if the user moved it while maximized (a title bar drag keeps the maximize), back to its size before, centred where it is now (2026-11-06). Remembered with its screen (`Core::maximized`). A resize ends the maximize (2026-11-06). An app's maximize button does the same, except at a zoom other than 1, where it is only answered (apps never move the camera). |
| Super+Shift+Enter, an app's `set_fullscreen` | `entry/windows.rs` `toggle_fullscreen`, `protocol/presentation.rs` (M9) | Protocol fullscreen on the pointer's screen: the screen shows only it, at the screen's logical size rounded down (F-PRESENT-1). Nothing on the Canvas moves (owner, 2026-10-31). Leaving: the size before (`FullscreenState.saved`); at its first windowed frame the window keeps its place if it is free at the size it draws, else it moves to the nearest free spot (it came back larger, or a window took its place); it never pushes (`returned_from_fullscreen`). A Super+drag out of fullscreen places it where the user lets go. |
| Super+right drag | `protocol/resize.rs` `begin_resize`/`resize_to`/`end_resize` | From the nearest corner; one configure outstanding at a time; the opposite corner stays; no other window moves. Super+Shift+right drag scales a cluster. |
| App's `set_maximized`/`set_minimized` | `protocol/toplevels.rs` `toplevel_request`, `client_minimize` | Minimize (2026-11-07): only the focused window and never a fullscreen one (Wine asks it of a game that lost the focus); it is hidden, still open and listed, until picked from the windows list or asked for by its app or a taskbar. Maximize: the compositor's maximize is Super+Enter. Asked before the window is shown (Firefox or Discord reopening maximized), it is granted at map (`maximize_opened`): the screen's room in the nearest space free of other apps' windows, not the screen in view over them (owner, 2026-11-07). |

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
| Super+right drag | Resize from the nearest corner; no other window moves, it may cover them (owner, 2026-11-06). |
| Touch: three fingers | Move or resize the window under them (see the touch design notes). |
| Super+Tab | As Alt+Tab on Windows, with the canvas as the preview (2026-11-28). A tap: the window used before (one asking for attention first). Super held: each Tab focuses the next window in the order of use, raises it and, with `FOCUS_CYCLE_NAVIGATE`, centres the camera on the pointer's screen on it as Super+Space does; Super+Shift+Tab steps back, both wrap round. Letting Super go keeps the chosen window and puts it first in the order of use, the others keeping theirs; until then the order does not change. Escape goes back to the window and camera of before the first Tab. Minimized windows come last and come back only when Super is let go; Steam notification windows are left out. A fullscreen window keeps its fullscreen state and steps aside. No thumbnails or overlay. |
| Super+1..9 / Super+Shift+1..9 | Focus window / cluster N (creation order), raise, centre the camera (zoom 1) / frame the cluster. |
| Super+Shift+G | Sort the focused window's cluster by app, sizes kept (below). |
| Super+M (`BIND_WINDOW_MENU`) | The window menu of the window under the pointer, else of the focused one. An app that asks for it on a right-click and opens its own menu on the same press gets the window menu only (2026-11-24). |
| Click | Focus and raise the window under the pointer. |
| Super+Space / Super+Shift+Space | Centre the focused window in the usable area at zoom 1 / at the current zoom. A window as large as the whole screen is lined up with it at zoom 1; only a window larger than the screen is zoomed out until it fits, with a 12 px margin (2026-11-06). Super+1..9, a two-finger double tap and TELEPORT_ON_OPEN centre the same way. |

A focus change sends a configure to the window losing and the one gaining focus (`activated`);
with held sizes it no longer changes their size.

## Cluster layouts

A cluster is the windows within 300 px of each other (`Core::clusters`). These are the only
times one window's change moves other windows, and each is the person's asking: a key or a shell
command (owner, 2026-11-21; `crates/infynitae-core/src/arrange.rs`). Nothing re-arranges by
itself: a window that closes or is minimized leaves its gap (owner, 2026-11-28).

| Asked by | What |
| :--- | :--- |
| `BIND_CLUSTER_LAYOUT` (no default key since 2026-11-23), `infynitae msg arrange grid [inst elem]` | The focused window's cluster (a dialog's: its parent's; nothing when the focused window is fullscreen or kept on top) sorted into a square grid (owner, 2026-11-28, in place of spiral, tiles and line): k windows in n x n slots, n = ceil(sqrt(k)) (1, 2x2, 3x3, 4x4, ...), filled row by row, the focused window first and the others in reading order; the slots left over stay empty (5 windows: 3x3, a row of three and a row of two). Every window keeps its size: no configure is sent. A column is as wide as its widest window, a row as high as its tallest, 12 px apart, each window centred in its slot. The grid starts at the cluster's top-left corner; the camera frames it on the pointer's screen, at zoom 1 at most. |
| Super+Shift+G (`BIND_CLUSTER_BY_APP`), `infynitae msg arrange app` | The focused window's cluster sorted by app: every window at its own size (nothing asked to resize), one row per app in the order the apps' first windows read, left to right, rows 12 px apart and as wide as they need (not squeezed into one view); the camera frames the result. |

Fullscreen, minimized, kept-on-top and notification windows and dialogs are left out; a dialog
moves with its parent. `CLUSTER_SORT` "app" only picks where a new window opens (First map,
above); "tidy", which closed the gap a window left, was removed on 2026-11-28 and is ignored in an
old config (`live/config-check` names it).

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
and each screen's camera centre and zoom. At login it launches each app, moves each new window of
a saved app to the next saved anchor of that app (unless a window open before covers it), asks it
for its saved size or maximizes it again on the first screen (`resize_element`, 2026-10-31), puts
it back fullscreen on the screen it had (windowed if that screen no longer exists), and puts the
camera centres and zoom back.
`SESSION_RESTORE=restart` (owner, 2026-11-21) does this only after the compositor restarted:
each snapshot holds the boot's id (`/proc/sys/kernel/random/boot_id`), and one from another boot
or without an id is removed at login instead, so after a reboot no app is reopened. `true`
reopens at every login, `false` saves nothing.

## Tests

`crates/infynitae-backend-headless/tests/`: `t1_window_states` (held sizes; maximized and
fullscreen windows across 1920x1080, 1366x768, 2560x1440 at 1/1.5/2, 3840x2160 at 2, portrait,
and every change between them; unplug; Super+Shift+Enter moves nothing), `t1_protocol_bounds`
(room, oversize fit), `t1_fit_constraints`, `t1_fit_bindings`, `t1_window_slots`,
`t1_window_drag`, `t1_fullscreen`, `t1_transform`, `t1_outputs`, `t1_cluster_layout` (the grid,
1 to 10 windows); the layouts and `CLUSTER_SORT` also in `crates/infynitae-core/src/arrange.rs`. Free space (new windows near
the cursor, self-resize, back from fullscreen in every way): `t1_window_states` too.
