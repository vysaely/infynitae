# C12.F9 — INV-ARCH-2 and C9: a test-class `unsafe` site in Core's integration tests — PROPOSAL

Status: **SUPERSEDED BY RELOCATION, NOT ADOPTED (2026-10-15).** By owner direction the counting
allocator moved unchanged to `crates/infynitae-backend-headless/tests/` on
`integration/inc-6-final-resolution-2026-10-15`, the Core integration-test exception was removed
from `enforcement/source-rules`, and INV-ARCH-2 stays as written. The owner decision of 2026-10-14
(allocator in a Core integration test) needs a formal correction; proposed text: "The OB-42
counting allocator is a test-class `unsafe` site in the headless backend's integration tests, a
`deny` crate (C9); Core and ipc forbid `unsafe` everywhere, their integration tests included. The
decision of 2026-10-14 placing it in a Core integration test is withdrawn." OB-42's "May be closed
by: T0 PRODUCT test in core" is then read as a T0 test of Core's API. The text below is kept as
drafted.

Original status: **PROPOSAL, NOT ADOPTED.** Drafted 2026-10-15 on the rehearsal branch
`rehearsal/inc-6-on-main-23752f1` (INC-6 `f1ef262` merged onto main `23752f1`). This file is
evidence, not authority. No living document is changed by it. Adoption needs the owner's approval
and then the layer process (a §C12.F9 section in FOUNDATION_ARCHITECTURE, the effective row in
FOUNDATION_INVARIANTS, the checker's layer list), as for C12.F5–C12.F8 and C12.F10.

The number C12.F9 was reserved for this layer on 2026-10-14 (FOUNDATION_ARCHITECTURE §C12.F10,
CHECKPOINT "C12.F9 (INV-ARCH-2 test-only allocator) is deferred to the INC-6 exit").

## The question

INV-ARCH-2 (C12, `615b0ed`, unchanged since): "`unsafe`: forbidden in core and ipc; elsewhere only
listed, justified sites with no authority effect". C9 implements "core" as
`#![forbid(unsafe_code)]` in `infynitae-core`.

OB-42 (INV-ALLOC-1) needs allocations of Core's hot paths to be counted. In Rust that needs a
`#[global_allocator]`, which needs `unsafe impl GlobalAlloc`. The owner approved on 2026-10-14 a
counting allocator in a Core integration test (`crates/infynitae-core/tests/t0_hot_path_allocations.rs`,
inventory rows U-002…U-004, class `test`). An integration test is a separate crate that links
Core's library and is never linked into it. Read literally, "forbidden in core" could cover that
crate too. INC-6 currently reads "core" as Core's library and records that reading as an open
review point (`PERFORMANCE_P1_REPAIR.md` §6). This layer would make the reading explicit.

## Proposed canonical row

| ID | Statement (canonical) | Owner | Class | Evidence | Tests (tier) | Trigger |
|---|---|---|---|---|---|---|
| **INV-ARCH-2** | `unsafe`: forbidden in core and ipc; elsewhere only listed, justified sites with no authority effect. **"Core" is the `infynitae-core` library, its examples, benches and build script; `infynitae-ipc` is forbidden everywhere. Core's integration-test crates may hold only sites listed in the inventory with class `test`; such a site never links into the library, never authorises production code and has no authority effect.** | workspace | D | C9; evidence/phase-f/INC-6/PERFORMANCE_P1_REPAIR.md | T0 lint + inventory review; `tools/verify-architecture` source rules and their seeded controls | owner decision 2026-10-14 (OB-42 counting allocator) |

## Proposed C9 change

The first bullet of C9 "`infynitae-core`: `#![forbid(unsafe_code)]`" is unchanged. One bullet is
added after it:

- Core's integration tests (`crates/infynitae-core/tests/`) are separate crates. A site there needs
  a `test` row in `docs/foundation/UNSAFE_INVENTORY.md` and carries items 1–5 below like any other
  site. No such site may exist in Core's library, examples, benches or build script, or anywhere in
  `infynitae-ipc`.

## Sealed text superseded (if adopted)

| Superseded | Replaced by |
|---|---|
| C12 INV-ARCH-2 "forbidden in core", silent on Core's integration-test crates | INV-ARCH-2 (C12.F9): "core" is the library and its examples, benches and build script; integration tests may hold listed `test`-class sites only |

## What this layer does not change

- `#![forbid(unsafe_code)]` in Core's library and in `infynitae-ipc`;
- the C9 requirements for every site (SAFETY note, narrow scope, inventory row, verification note,
  no authority effect);
- the adapter and backends (`deny` + listed sites);
- INV-ALLOC-1 and OB-42: the ratchet is not the zero-allocation criterion, and OB-42 stays OPEN.

## Present enforcement (on the rehearsal branch)

- `tools/verify-architecture` (from INC-6, `enforcement/source-rules`): Core's integration tests are
  the one place of a `forbid` crate where a listed `test` site is accepted. Six seeded controls (one
  positive, five negative: unlisted site in a Core test; production-class row for it; listed site in
  a Core example; listed site in a Core library module named `tests`; listed site in an ipc test).
  Result on `rehearsal/inc-6-on-main-23752f1`: PASS.
- The only sites: U-002…U-004 in `t0_hot_path_allocations.rs`.

## Alternative (no layer)

If the owner reads INV-ARCH-2 to cover Core's integration tests, C12.F9 is not adopted. Instead,
the counting-allocator test moves to a backend crate's tests (a `deny` crate, where a listed `test`
site is already permitted). The Core-test exception in `tools/verify-architecture`'s source rules is
then reverted. Nothing else depends on it. The number C12.F9 would then stay unused or be released by
an owner note.

## Adoption steps (not done)

1. Owner decision: adopt this reading, or take the alternative.
2. If adopted: add §C12.F9 to FOUNDATION_ARCHITECTURE (after §C12.F10, since layers are ordered by
   adoption), update the INV-ARCH-2 entry in FOUNDATION_INVARIANTS (wording, Layers C12 → C12.F9,
   the header's layer list) and the checker's layer list, add the C9 bullet, and run
   `tools/check-foundation-docs` and its self-test.
3. Record the adoption in CHECKPOINT. This does not ratify INC-6.
