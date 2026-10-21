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
| Super + Enter | viewport fit toggle | **implemented 2026-10-21** (owner decision 2026-10-21: "super enter fit"): fits the window under the pointer; pressed on exactly that fit, restores. Stateless (the camera is compared with the fit it would make). Logo+F / Logo+Shift+F (C12.F17) stay. FocusNext, which held Logo+Enter as a harness fixture, moved to **Logo+N** (not in the owner's list; open to the owner's choice) |
| Super + Shift + Enter | native fullscreen | **implemented 2026-10-21** (owner decision 2026-10-21): the focused window's fullscreen toggles on the pointer's output, through the client-request path (M9) |
| **Super + Delete** | **close** | **implemented 2026-10-21** (`xdg_toplevel.close` to the focused window; exact modifiers; not during an interaction) |
| Super + Tab | overview | not implemented (shell) |
| Super + Space | recenter camera on the focused element | not implemented (`Navigate` to the element exists in INC-11) |
| Super + Q | terminal | not implemented |
| Super + W | launcher/panel | not implemented (shell) |
| Super + E | file manager | not implemented |
| PageUp / Super + PageUp | regional / fullscreen screenshot | not implemented (needs screen capture) |
| Super + / . , | reserved (window/quick/settings) | reserved |
| modifier + wheel; pinch | zoom around the cursor | not implemented (OB-53 CameraZoom part) |
