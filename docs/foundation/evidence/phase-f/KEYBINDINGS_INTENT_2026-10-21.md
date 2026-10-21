# Owner keybinding intent (recorded 2026-10-21)

Status: **OWNER INTENT**, stated in the owner's INC-2 brief of 2026-10-05 ("KNOWN INTERACTION
INTENT … future product interactions, NOT INC-2 requirements") and confirmed for Super+Delete on
2026-10-21 ("didnt i choose super + del ?"). Recorded here because no repository document carried
it (`audit/phase-f-inc4-review/KEYBOARD_AND_FOCUS.md:29`: "Canonical documents define no product
key bindings"). Not normative; each binding is implemented by the change that names it.

| Binding | Intent | State |
|---|---|---|
| Super + Middle Mouse | pan the canvas/camera | implemented (INC-3) |
| Super + 1..9 | spatial/window slots | not implemented |
| Super + Enter | viewport fit toggle | **implemented 2026-10-21** (owner decision 2026-10-21: "super enter fit"): fits the window under the pointer; pressed on exactly that fit, restores. Stateless (the camera is compared with the fit it would make; every refusal is the M8 action's own). **Super+F and Super+Shift+F removed** (owner, 2026-10-21: "i dont like duplicates so just remove the f"); deeper restores (several steps back through the 256-entry history) remain reachable through the shell's `viewport_restore` |
| Super + Shift + Enter | native fullscreen | **implemented 2026-10-21** (owner decision 2026-10-21): the focused window's fullscreen toggles on the pointer's output, through the client-request path (M9) |
| **Super + Delete** | **close** | **implemented 2026-10-21** (`xdg_toplevel.close` to the focused window; exact modifiers; not during an interaction) |
| Super + Shift + Delete | end the app (force close) | **implemented 2026-10-21** (proposed 2026-10-21, owner: "ig ?"): the focused window's client — its process pinned by a pidfd while connected, the connection cut, then SIGKILL; only a process of this user, never the compositor's own; an X11 window (client xwayland-satellite, shared by every X11 window) is asked to close instead |
| Super + Tab | focus the next window (cycle) | **implemented 2026-10-21** (owner, 2026-10-21: "lets try super tab for cycle and overiew super shift tab"): FocusNext, an x10 harness fixture formerly on Logo+Enter (INC4-F04) |
| Super + Shift + Tab | overview | reserved (owner, 2026-10-21); not implemented (shell); not bound in Core |
| Super + Space | recenter camera on the focused element | not implemented (`Navigate` to the element exists in INC-11) |
| Super + Q | terminal | not implemented |
| Super + W | launcher/panel | not implemented (shell) |
| Super + E | file manager | not implemented |
| PageUp / Super + PageUp | regional / fullscreen screenshot | not implemented (needs screen capture) |
| Super + / . , | reserved (window/quick/settings) | reserved |
| modifier + wheel; pinch | zoom around the cursor | not implemented (OB-53 CameraZoom part) |
