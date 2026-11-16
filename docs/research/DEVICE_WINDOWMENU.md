# DEVICE_WINDOWMENU: Window menu with "Keep on top"

Date: 2026-11-16
Branch: `agy/d5-window-menu-float`

---

## 1. What changed

- `crates/infynitae-core/src/lib.rs:208-223`, `517-519`, `627`, `1258-1267`: Added `ShellCommand::KeepOnTop`, `MinimizeElement`, `MaximizeElement` and `CloseElement`, plus `window_menu: Option<(stage::ElementId, (f64, f64))>` with `Core::window_menu` and `Core::take_window_menu`.
- `crates/infynitae-core/src/protocol.rs:406-415`, `901-905`, `2338-2370`, `3645-3665`: Advertised `window_menu: true` in `Core::toplevel_capabilities`, added `Core::is_maximized`, and added `Core::show_window_menu` projecting the window-local menu coordinate through the active output's camera view to output-logical screen coordinates.
- `crates/infynitae-core/src/input/keys.rs:167-168`, `322-327`, `393`: Added `KeyAction::WindowMenu`, `("WINDOW_MENU", KeyAction::WindowMenu)` in `ACTION_NAMES`, and mouse button names (`BTN_LEFT`/`MOUSE_LEFT`, `BTN_RIGHT`/`MOUSE_RIGHT`, `BTN_MIDDLE`/`MOUSE_MIDDLE`) in `KEY_NAMES` while leaving the default `Super+BTN_RIGHT` binding as `WindowDrag::Resize` (`crates/infynitae-core/src/input/bindings.rs:29`).
- `crates/infynitae-core/src/entry.rs:828-838`, `2474-2487`, `3002-3055`: Handled `KeyAction::WindowMenu` on key or mouse button chords, and executed `ShellCommand::KeepOnTop` (calling `set_on_top` and emitting `Effect::KeptOnTop` for `remember_for_app`), `MinimizeElement`, `MaximizeElement` and `CloseElement`.
- `crates/infynitae-wayland/src/handlers.rs:796-808`: Forwarded `XdgShellHandler::show_window_menu` locations to `Core::show_window_menu` via `convert::local_point`.
- `crates/infynitae-ipc/src/model.rs:251-271`, `crates/infynitae-ipc/src/protocol.rs:249-276`, `crates/infynitae-ipc/src/server.rs:240-248`, `374-387`: Added `"on_top"` to element snapshots, added `window_menu_event` (`"event": "window_menu"` with `"element"`, `"position"`, `"on_top"`, `"maximized"`, `"fullscreen"`), broadcast it immediately to subscribed shell clients, and parsed `"keep_on_top"`, `"minimize"`, `"maximize"` and `"close"` requests.
- `crates/infynitae/src/msg.rs:373-438`: Added `keep-on-top`, `minimize`, `maximize`, `fullscreen` and `close` subcommands to `infynitae msg`.
- `live/edge-apps/src/ui.rs:60-61`, `199-204`, `643-655`, `live/edge-apps/src/cards/tray_menu.rs:231-268`, `321-331`: Added `ui::Icon::Check` (drawn in `th.accent_text` when active) and `tray_menu::draw_menu_row` shared between the tray menu and the window menu.
- `live/edge-apps/src/card.rs:218-219`, `live/edge-apps/src/boxes.rs:40-51`, `live/edge-apps/src/shell.rs:103-219`, `597-626`, `989-997`, `1257-1285`, `1470-1484`, `1574-1635`, `1901-1933`: Implemented `WindowMenu` and `WindowMenuAction` (`Keep on top` with check mark when on, `Minimize`, `Maximize`/`Restore`, `Fullscreen`, `Close`), JSON event parsing from `shell.sock`, drawing, pointer/keyboard interaction, and `Action::ShellCommand`.
- `live/edge-apps/src/wayland.rs:508-510`, `762-775`, `1542-1576`, `live/edge-apps/src/main.rs:40-53`, `220-245`: Subscribed `infynitae-edge-apps` to `$XDG_RUNTIME_DIR/infynitae/shell.sock` for `"window_menu"` events, sent `Action::ShellCommand` lines back over the socket, and added the `INFYNITAE_EDGE_RENDER_WINDOW_MENU` / `--render window-menu` test hook.
- `live/edge-apps/src/cards/settings_rows.rs:173-175`, `live/edge-apps/src/cards/settings_windows.rs:1-2`, `141-149`, `live/edge-apps/src/cards/settings_keys.rs:231-236`, `live/edge-apps/src/cards/settings_all.rs:1527-1535`, `1711-1744`, `config/config:882`, `docs/EDGE_SETTINGS.md:271`: Moved `KEEP_ON_TOP` out of `FILE_ONLY` onto the Windows settings card as a `Control::Text` row, and added `WINDOW_MENU` to the Keys settings card.
- `crates/infynitae-ipc/tests/t1_shell.rs:1030-1089`, `crates/infynitae-backend-headless/tests/t1_keep_on_top.rs:224-368`, `crates/infynitae-backend-headless/tests/t1_toplevel_requests.rs:428-432`, `tools/verify-test-efficacy:694-697`, `live/edge-apps/src/shell.rs:2812-2886`: Added IPC, headless and edge-apps tests covering `show_window_menu`, `BIND_WINDOW_MENU`, `keep_on_top` camera pan following and `remember-window`, and window menu items/labels/actions.

---

## 2. Found but not changed (ranked)

1. **`KEEP_ON_TOP` config reload does not unpin windows already mapped** (`crates/infynitae-wayland/src/lib.rs:1399`, `crates/infynitae-core/src/protocol.rs:2357-2363`) — **Size: S**.
   `set_keep_on_top` updates `self.keep_on_top`, which is only checked when a window next commits metadata (`protocol.rs:1196`), so editing `KEEP_ON_TOP` in the Windows settings card does not immediately pin or unpin already-open windows until their title or app id changes. Fix: re-evaluate `named_on_top` across mapped elements inside `set_keep_on_top`.
2. **`launch.sh remember-window` assumes double-quoted `KEEP_ON_TOP="..."` lines** (`live/launch.sh:599-620`) — **Size: S**.
   When `settings.sh set KEEP_ON_TOP ...` writes single quotes (`KEEP_ON_TOP='...'`), `remember-window`'s `sed`/`grep` pattern looking for `KEEP_ON_TOP="[^"]*"` appends a second line instead of updating the single-quoted one in place. Fix: match both single-quoted and double-quoted assignments in `live/launch.sh:599-620`.
3. **Multi-output window menu placement uses the first output in `edge-apps`** (`live/edge-apps/src/wayland.rs:615-617`, `crates/infynitae-ipc/src/model.rs:256-271`) — **Size: S**.
   The `"window_menu"` IPC event carries output-logical `[x, y]` coordinates without an output name, and `edge-apps` draws its active surface on each configured output. Fix: include the output name in `window_menu_event` and open the menu only on that output's layer surface when multiple monitors are connected.

---

## 3. What needs real hardware or a live desktop session to confirm

- Right-clicking a client-drawn title bar in GTK4, Firefox and Qt6 apps in a live DRM session to confirm client `xdg_toplevel.show_window_menu` coordinates align with the cursor under fractional output scale (`OUTPUT_SCALE` != 1).
- Clicking menu items in the live `infynitae-edge-apps` layer surface over a running window and verifying pointer/keyboard focus return after the menu closes.
