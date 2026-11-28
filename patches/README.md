# Vendored dependency patches

`vendor/smithay-0.7.0` is the published `smithay` 0.7.0 with `0001` to `0008` applied in order
(`patch -p1`, no offset or fuzz; checked 2026-11-26 against the crate from crates.io: the trees
differ only in `Cargo.lock`, which the crate carries, and `.cargo-ok`). Every vendored file's
hash is pinned in `enforcement/smithay-0.7.0.sha256` and checked by `tools/verify-architecture`.
Each patch's header says what failed, the decision and its date, and the upstream state.

## Smithay 0.7.0

Lines: added and removed lines of the change (the patch file's own length in brackets).
Upstream: what Smithay `master` had on 2026-11-26; no Smithay release after 0.7.0 exists. None of
the patches has been sent: sending is an outward action left to the owner (OB-11). On `master`:
`patch -p1 --dry-run -F3` on `master` at `54cf9c1` (2026-11-24); applying is not building, and no
patch was built against `master`.

| # | Patch | Files | Lines | Reason | Upstream | On `master` |
| --- | --- | --- | --- | --- | --- | --- |
| P1 | `0001-seat-leave-deactivate-only-active-constraint.patch` | `src/wayland/seat/pointer.rs` | +3 −1 (26) | `leave` deactivated a constraint that was not active: a second `unlocked`/`unconfined` (W-SM-4c; D-B12-PIN, 2026-10-07) | on `master`: [PR #2144](https://github.com/Smithay/smithay/pull/2144) (`e3d461a`, 2026-08-26) has the same guard; nothing to send | fails: already there |
| P2 | `0002-drm-async-page-flip.patch` | `src/backend/drm/{mod.rs, compositor/mod.rs, surface/mod.rs, surface/atomic.rs, surface/legacy.rs}` | +497 −24 (732) | async page flips, so fullscreen games can tear (2026-10-26; primary FB only, 2026-11-21) | not on `master`; ready to send after a build against `master`. A new API in the DRM backend: open an issue first | applies, hunk 10 with fuzz 3 |
| P3 | `0003-egl-lose-context-on-reset.patch` | `src/backend/egl/context.rs` | +27 −8 (116) | a GPU reset or full GPU memory loses the context instead of aborting the compositor (2026-10-26) | not on `master`; ready to send after a build against `master`, together with what stays of P4 | applies, hunk 2 with fuzz 2 |
| P4 | `0004-egl-context-priority.patch` | `src/backend/egl/context.rs` | +33 −0 (55) | the compositor's context at high GPU priority, granted priority read back (2026-10-31) | half on `master`: [PR #2202](https://github.com/Smithay/smithay/pull/2202) (`a96a4fb`, 2026-10-26) reads the granted priority back (`get_context_priority`, ours is `priority`); the constructor with priority and lose-on-reset needs P3, so it goes with P3 | applies with fuzz 1 |
| P5 | `0005-remove-unused-tracing-error-imports.patch` | `src/backend/drm/surface/gbm.rs`, `src/backend/session/libseat.rs`, `src/backend/winit/mod.rs`, `src/input/keyboard/mod.rs`, `src/wayland/seat/keyboard.rs` | +5 −5 (74) | five unused `tracing::error` imports, a warning each on every build (2026-11-13) | on `master`: [PR #1839](https://github.com/Smithay/smithay/pull/1839) (`07f9db5`, 2025-10-07); nothing to send | fails: already there |
| P6 | `0006-xdg-unconfigured-buffer-error.patch` | `src/wayland/shell/xdg/{mod.rs, handlers/surface.rs, handlers/surface/popup.rs, handlers/surface/toplevel.rs}` | +241 −88 (498) | a buffer before `ack_configure` gets xdg error 3, not 1: backport of upstream `641d2ad`, keeping `ensure_configured` as a check that posts nothing (W-SM-18, 2026-11-13) | on `master`: [PR #1817](https://github.com/Smithay/smithay/pull/1817) (`641d2ad`, 2025-09-17); our one difference, `ensure_configured` kept, serves only our adapter; nothing to send | fails: already there |
| P7 | `0007-pointer-constraints-expose-lifetime.patch` | `src/wayland/pointer_constraints.rs` | +2 −1 (24) | `PointerConstraint::lifetime` made public, so the adapter reports oneshot or persistent (W-SM-21, 2026-11-13) | still private on `master`; ready to send as it is | applies unchanged (`git apply --check`) |
| P8 | `0008-keyboard-leave-clears-held-modifiers.patch` | `src/input/keyboard/mod.rs` | +22 −0 (54) | the window losing the keyboard is told no modifier is held (owner report 2026-11-15) | not on `master`: its `set_focus` sends `modifiers` only to the new focus (read 2026-11-26); ready to send after a build against `master` | applies, with fuzz 3 and 2 |

Total: 830 lines added, 127 removed. Section 6.3 of `docs/POLISH_PLAN.md` sets about ten patches
or 600 lines as the point to move to a newer Smithay instead of adding patches; the set is past
the line count.

## Other projects

| Patch file | Patch lines | Files touched | Why | Upstream state |
| --- | --- | --- | --- | --- |
| `xwayland-satellite-0001-answer-minimize.patch` | 146 | `src/xstate/mod.rs`, `tests/integration.rs` | Answer `WM_CHANGE_STATE` (`Iconic`) on X11 windows and restore `Normal` on focus or when already focused (`fa7e131`, 2026-11-02). | Not sent. Applies unchanged to `main` at `79aaba6` (2026-11-16, `git apply --check`), which has no answer to `WM_CHANGE_STATE` either. |

## Taking an upstream fix

`tools/upstream-check` lists newer releases and the security-relevant entries since ours. For
Smithay, until a release after 0.7.0 exists, a fix comes in as one more patch:

1. Get the change: `https://github.com/Smithay/smithay/pull/<number>.patch`, or
   `git format-patch -1 <commit> --stdout` in a Smithay clone.
2. Save it as `patches/00NN-<what-it-fixes>.patch` with the header the others have (what failed,
   decision and date, upstream status), and apply it: `cd vendor/smithay-0.7.0 && patch -p1 <
   ../../patches/00NN-....patch`. A hunk that needs fuzz or fails is backported by hand, as P6 was.
3. Re-pin the hashes. The whole tree: `cd vendor/smithay-0.7.0 && find . -type f | sed 's|^\./||'
   | LC_ALL=C sort | xargs -d '\n' sha256sum > ../../enforcement/smithay-0.7.0.sha256` (gives the
   file as it is when nothing changed; checked 2026-11-26). Each patched file also has its own
   hash in `tools/verify-architecture` (`AUTHORIZED_POINTER_SHA256` for P1, the
   `AUTHORIZED_P*_SHA256` tables by file: P4 is under P3 and P8 under P5, the same files): set the
   touched files' new `sha256sum` there, add a file not yet listed, and name the patch in the
   comment above them.
4. Add the row above, then run `tools/verify-architecture` and the tests the files cover.

For xwayland-satellite, a newer release is `TAG` and `COMMIT` in `live/build-satellite.sh`; the
patch must still apply (`git apply --check`), and the `--ignore` list of the "Advisories" CI job,
read for the old commit, is read again for the new one.

## Plan: Smithay as a normal crate

Today the repository carries all 270 files of Smithay 0.7.0 so that 8 patches can sit on them.
The aim is to depend on a Smithay release from crates.io and keep a `[patch]` only for what
upstream refuses:

1. P1, P5 and P6 are on `master`, and P4's read-back is: they go away with the first release
   after 0.7.0.
2. Send P7 (applies unchanged), P8, and P3 with the rest of P4; open an issue about P2 before a
   pull request, since it adds API to the DRM backend.
3. On that release: `smithay = "=<version>"` in the crates, no vendored tree, and the
   `smithay` entry dropped from `ignore` in `.github/dependabot.yml`. `tools/verify-architecture`
   then checks the version and the crates.io source in `Cargo.lock` in place of the vendored
   path and the file hashes.
4. What upstream refused or has not released stays a `[patch.crates-io]`: a tree of that release
   with only those patches, by path as today, or by `git` with a pinned `rev`, which keeps the
   files out of this repository. Each patch keeps its header and its row here.
