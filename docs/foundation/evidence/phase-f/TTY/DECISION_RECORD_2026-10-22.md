# Console (TTY) backend — decision record (2026-10-22)

**Decided by the owner on 2026-10-22.** After an explanation of each point and a check that
Hyprland and the others work the same way, the owner answered: "nvm just keep it as is and build
the thing". That approves **T1–T9 as proposed** in `DECISIONS_PROPOSAL_2026-10-21.md` (copied
unchanged beside this record). The key bindings stay as they are; the owner considered changing
them for a 60% keyboard and decided not to.

| # | Decision |
|---|---|
| T1 | New crate `infynitae-backend-drm`; `--tty`, or automatic when neither `WAYLAND_DISPLAY` nor `DISPLAY` is set |
| T2 | libseat for device access and VT switching |
| T3 | GLES on GBM/EGL through Smithay's `DrmCompositor` |
| T4 | libinput on the seat; keyboard layout from `XKB_DEFAULT_*` |
| T5 | Every connected connector at its preferred mode, on the primary GPU (hotplug: see the implementation record) |
| T6 | Ctrl+Alt+Backspace quits; Ctrl+Alt+F1…F12 switch VT, taken before Core |
| T7 | The crate names only `smithay`; drm, gbm, input, libseat and udev come through Smithay's features; Core stays dependency-free |
| T8 | The compositor draws the cursor: the client's surface, or a built-in arrow |
| T9 | Pure parts tested; the first real run is the owner's (standing rule: no DRM/KMS/VT for the assistant); EXPERIMENTAL |

Consequence: U-007 in `UNSAFE_INVENTORY.md`. Smithay's EGL display and GLES renderer
constructors are `unsafe`, which is inherent to T3.
