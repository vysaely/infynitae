# Unsafe-inventory enforcement — integration record

Status: **integrated on local canonical `main` (owner authorization 2026-10-13); not pushed.**
This file is evidence, not authority. C9 and INV-ARCH-2 are unchanged; the change is to the tool
that enforces them.

## 1. Commit

`804ab65` `enforcement(unsafe): every unsafe site must match an inventory entry (C9)`, one commit
on `ede1541`, fast-forwarded, followed by this record.

## 2. What changed

Before: the `unsafe-policy` rule of `enforcement/source-rules` accepted any `unsafe` in the
adapter and the backends as soon as `docs/foundation/UNSAFE_INVENTORY.md` existed. It never read
the file.

Now:

- a **site** is a function, method or `macro_rules` definition of one file (or `<file>` for file
  scope) together with the number of `unsafe` tokens and `unsafe_code` relaxations inside it;
- every site needs **exactly one** inventory row with the same file, item and count;
- a row is classed `test` (`tests/`, `benches/`, `examples/`) or `production` (`src/`) and never
  authorises the other class;
- a row records purpose, safety preconditions, ownership assumptions, cleanup guarantees and
  verification; an empty cell fails;
- unlisted, duplicate, ambiguous, stale, malformed or incomplete rows fail;
- a `forbid` crate fails for any site, listed or not;
- an inventory that exists but lists nothing authorises nothing.

At this commit the workspace has no `unsafe` site and no inventory file.

## 3. Seeded controls (`tools/verify-architecture`)

19 controls on temporary copies of the tree, one positive (a listed test site passes) and 18
negative: an inventory without entries; unlisted `unsafe` in the adapter, the nested backend and
the headless backend beside a valid inventory; unlisted `allow(unsafe_code)` in the adapter; a
count mismatch; a different function of a listed file; the same site listed twice; an id used
twice; a stale row; a test-class row for production code and the reverse; `unsafe` in a macro
definition; `unsafe` in a macro invocation outside the listed function; `unsafe impl` at file
scope listed under a function name; a malformed row; a row with an empty safety cell; a listed
site in Core.

## 4. Limitation: macros defined in other crates

The rule reads source tokens. It sees `unsafe` written in this workspace, including inside local
macro definitions and inside the arguments of any macro invocation. It does **not** see `unsafe`
that exists only after expanding a macro defined in another crate, because the token is in that
crate's source. Such a site cannot be listed and is not counted. Only review of the dependency
covers it; whether rustc's `unsafe_code` lint reports such an expansion is not relied on and was
not tested. A search of the workspace for commonly unsafe-expanding macros (`pin_mut!`,
`offset_of!`, `container_of!`, `lazy_static!`, `thread_local!`) found none; that search is not a
proof of absence.
Closing the gap would need a check on expanded code (for example `cargo expand` output), which
is not implemented.

## 5. Not claimed

No `unsafe` site is authorised by this integration. The windowless GLES test and its inventory
row (`U-001`) remain on an experimental branch.
