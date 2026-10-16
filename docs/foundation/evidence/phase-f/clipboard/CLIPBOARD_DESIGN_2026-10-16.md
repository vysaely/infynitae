# Clipboard slice — design, limits and evidence (2026-10-16)

Status: **PROPOSAL for the owner** (branch `feature/clipboard-2026-10-16`, from `main` 23752f1).
No new dependency (Smithay 0.7.0's `wayland_frontend` already contains the protocols). No
`unsafe`. Not merged.

## 1. What is offered

| Global | Version | Notes |
|---|---|---|
| `wl_data_device_manager` | 3 (Smithay) | selection (clipboard); drag-and-drop requests are accepted by Smithay's defaults only with an implicit pointer grab — DnD is **not** validated against Core's pointer ownership and is not claimed |
| `zwp_primary_selection_device_manager_v1` | 1 | the middle-click selection, same focus rule |
| `zwlr_data_control_manager_v1` / `ext_data_control_v1` | — | **not offered** (§4) |

## 2. Policy

- **Authorization follows the keyboard focus** (which follows Core's ActiveElementFocus):
  only the focused client is offered the selection, and only it may set one (Smithay refuses
  `set_selection` from a client without keyboard focus). On a focus change the newly focused
  client is sent the current offer (`set_data_device_focus` / `set_primary_focus` in
  `SeatHandler::focus_changed`).
- **No daemon, no storage.** The compositor never reads selection contents: a paste is a pipe
  from the source client to the receiving client. Nothing is persisted or logged.
- **Ownership.** A new selection from the focused client replaces the old one; the old source
  is cancelled. The owner client is recorded per selection kind.
- **Cleanup.** When the owning client disconnects, its selection is cleared and the focused
  client is told (`selection(null)`) — `WaylandState::clear_orphaned_selections`, run after
  every dispatch. A transfer whose source client dies ends with EOF at the receiver, never a
  hang (the receiver holds the only remaining write end once the source's fd is gone).

## 3. Known limits

- A source **object** destroyed while its client keeps running stays offered until the next
  selection; a paste then reads nothing (EOF). Smithay 0.7 keeps the source's provider
  `pub(crate)`, so the adapter cannot observe that destruction. Harmless (no data, no hang) but
  not exact protocol hygiene.
- No serial validation of `set_selection` beyond Smithay's focus check (Smithay 0.7 does not
  check the serial).
- Clipboard-manager compatibility (persisting a selection after its owner quits) is **not**
  provided and not claimed: it requires data-control (§4).

## 4. Data-control (assessed, not implemented)

`zwlr_data_control_manager_v1` / `ext_data_control_v1` let any client read and set the
selection without focus — what clipboard managers and `wl-copy`/`wl-paste` without a window
use. Offering it to every client would defeat §2's focus authorization (any client could read
every copy). If wanted, it should be a separate slice: off by default, enabled per session by an
explicit option, ideally limited to clients the session started. Owner decision; listed in the
report.

## 5. Evidence

T1 (`crates/infynitae-backend-headless/tests/t1_clipboard.rs`, production `WaylandState`, two
clients over real sockets): copy in one client and paste in another; an unfocused client
neither sees nor sets the selection; a new copy takes the selection over (old source
cancelled); a quitting owner clears it; the source client dying mid-transfer ends it with EOF;
stale offers and an invalid drag are harmless; the primary selection follows the focus.
8 tests pass; 3 seeded controls (clipboard focus not following the keyboard focus, primary-selection focus not following it, an orphaned selection not cleared) DETECTED
by `tools/verify-test-efficacy`.

Real tools on the headless build in a private namespace (private XDG_RUNTIME_DIR, no D-Bus, no
access to the desktop session): `wl-copy --foreground` (it maps a surface and so gets the focus)
set the selection; `wl-paste` printed `infynitae clipboard`, exit 0; `wl-paste --list-types`
listed the offered types. A backgrounded `wl-copy` without focus is (correctly) refused — that
is the focus rule, not a failure.

GTK4 (`zenity --info`, the GTK4 app available here): before this slice GTK4 refused to start
(no `wl_data_device_manager`); with it, zenity binds `wl_data_device_manager` v3, receives an
activated `xdg_toplevel` configure and attaches a buffer (headless, `GSK_RENDERER=cairo`, no
D-Bus). Copy/paste *inside* a GTK app needs input injection and is left to the nested run.
