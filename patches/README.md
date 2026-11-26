# Vendored dependency patches

`vendor/smithay-0.7.0` is the published `smithay` 0.7.0 with `0001` to `0008` applied in order
(`patch -p1`, no offset or fuzz; checked 2026-11-26 against the crate from crates.io: the trees
differ only in `Cargo.lock`, which the crate carries, and `.cargo-ok`). Every vendored file's
hash is pinned in `enforcement/smithay-0.7.0.sha256` and checked by `tools/verify-architecture`.
Each patch's header says what failed, the decision and its date, and the upstream state.

## Smithay 0.7.0

Lines: added and removed lines of the change (the patch file's own length in brackets).
Upstream: what Smithay `master` had on 2026-11-26; no Smithay release after 0.7.0 exists.

| # | Patch | Files | Lines | Reason | Upstream |
| --- | --- | --- | --- | --- | --- |
| P1 | `0001-seat-leave-deactivate-only-active-constraint.patch` | `src/wayland/seat/pointer.rs` | +3 −1 (26) | `leave` deactivated a constraint that was not active: a second `unlocked`/`unconfined` (W-SM-4c; D-B12-PIN, 2026-10-07) | the same guard is on `master`; not sent |
| P2 | `0002-drm-async-page-flip.patch` | `src/backend/drm/{mod.rs, compositor/mod.rs, surface/mod.rs, surface/atomic.rs, surface/legacy.rs}` | +497 −24 (732) | async page flips, so fullscreen games can tear (2026-10-26; primary FB only, 2026-11-21) | not on `master`; not sent |
| P3 | `0003-egl-lose-context-on-reset.patch` | `src/backend/egl/context.rs` | +27 −8 (116) | a GPU reset or full GPU memory loses the context instead of aborting the compositor (2026-10-26) | not on `master`; not sent |
| P4 | `0004-egl-context-priority.patch` | `src/backend/egl/context.rs` | +33 −0 (55) | the compositor's context at high GPU priority, granted priority read back (2026-10-31) | `master` supports `EGL_IMG_context_priority`, API not compared; not sent |
| P5 | `0005-remove-unused-tracing-error-imports.patch` | `src/backend/drm/surface/gbm.rs`, `src/backend/session/libseat.rs`, `src/backend/winit/mod.rs`, `src/input/keyboard/mod.rs`, `src/wayland/seat/keyboard.rs` | +5 −5 (74) | five unused `tracing::error` imports, a warning each on every build (2026-11-13) | removed on `master` |
| P6 | `0006-xdg-unconfigured-buffer-error.patch` | `src/wayland/shell/xdg/{mod.rs, handlers/surface.rs, handlers/surface/popup.rs, handlers/surface/toplevel.rs}` | +241 −88 (498) | a buffer before `ack_configure` gets xdg error 3, not 1: backport of upstream `641d2ad`, keeping `ensure_configured` as a check that posts nothing (W-SM-18, 2026-11-13) | PR #1817, on `master` |
| P7 | `0007-pointer-constraints-expose-lifetime.patch` | `src/wayland/pointer_constraints.rs` | +2 −1 (24) | `PointerConstraint::lifetime` made public, so the adapter reports oneshot or persistent (W-SM-21, 2026-11-13) | still private on `master`; not sent |
| P8 | `0008-keyboard-leave-clears-held-modifiers.patch` | `src/input/keyboard/mod.rs` | +22 −0 (54) | the window losing the keyboard is told no modifier is held (owner report 2026-11-15) | not verified; not sent |

Total: 830 lines added, 127 removed. Section 6.3 of `docs/POLISH_PLAN.md` sets about ten patches
or 600 lines as the point to move to a newer Smithay instead of adding patches; the set is past
the line count.

## Other projects

| Patch file | Patch lines | Files touched | Why | Upstream state |
| --- | --- | --- | --- | --- |
| `xwayland-satellite-0001-answer-minimize.patch` | 146 | `src/xstate/mod.rs`, `tests/integration.rs` | Answer `WM_CHANGE_STATE` (`Iconic`) on X11 windows and restore `Normal` on focus or when already focused (`fa7e131`, 2026-11-02). | Local patch for `xwayland-satellite`. |
