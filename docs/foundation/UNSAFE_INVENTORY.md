# Unsafe inventory (C9, INV-ARCH-2)

Every `unsafe` site of the adapter and the backends — production code, integration tests, benches
and examples — is listed here.
`tools/verify-architecture` matches each site against exactly one row (file, enclosing item,
number of sites, class) and fails on any site that is not listed, on any row without a site, and
on a row of the wrong class. A `test` row never authorises `production` code.

`infynitae-core` and `infynitae-ipc` forbid `unsafe` everywhere — library, integration tests,
examples, benches and build script (INV-ARCH-2 as written); no row can change that.

History: the OB-42 counting allocator was first placed in a Core integration test with an
exception for Core's integration tests (owner decision of 2026-10-14). On 2026-10-15, by owner
direction, it moved unchanged to the headless backend's tests (a `deny` crate, where a listed
`test` site is permitted) and the exception was removed; layer C12.F9, drafted to legitimise the
exception, is not adopted. The 2026-10-14 decision was corrected by the owner on 2026-10-16 (C12.F9
withdrawn; `evidence/phase-f/INC-6/ADOPTION_RECORD_2026-10-16.md`).

Production sites: **none.**

| id | file | item | sites | class | purpose | safety preconditions | ownership assumptions | cleanup guarantees | verification |
|---|---|---|---|---|---|---|---|---|---|
| U-002 | `crates/infynitae-backend-headless/tests/t0_hot_path_allocations.rs` | `<file>` | 1 | test | `unsafe impl GlobalAlloc for Counting`: the test binary's global allocator, which counts the allocations made on a thread while that thread counts (OB-42 ratchet, INV-ALLOC-1). | The implementation upholds `GlobalAlloc`'s contract because every method forwards its arguments unchanged to `std::alloc::System` and returns its result; `realloc` and `alloc_zeroed` are the trait's defaults, which call these. The counter is a `thread_local!` `Cell<Option<u64>>` with a constant initialiser and no destructor: reading or writing it neither allocates nor unwinds, and `try_with` tolerates a thread being torn down. | Only this test binary uses the allocator; Core's library and every other binary keep the system allocator. No Core state, port or authority is touched. | Nothing to release: the allocator owns no memory; every block belongs to `System`. | `hot_path_allocations_do_not_grow`: the counts are deterministic, equal at 10, 100 and 1,000 windows, and equal to those of `main` (`ccb82c5`) measured with the same file; the same file fails on `40ddd7d` (P-1). On the INC-6 × INC-7 integration (2026-10-15) the counts equal those of `main` `23752f1` (ratified INC-7) measured with the same file. On `integration/inc-6-final-resolution-2026-10-15` the test asserts 0 on every path (OB-42), from the headless backend's tests. |
| U-003 | `crates/infynitae-backend-headless/tests/t0_hot_path_allocations.rs` | `Counting::alloc` | 2 | test | Count the allocation, then allocate from `System`. | The caller of `alloc` guarantees a `layout` of non-zero size (`GlobalAlloc::alloc`'s contract); it is passed to `System.alloc` unchanged. | The returned block belongs to the caller, as from `System`. | Freed through `Counting::dealloc`, i.e. by `System`. | As U-002. |
| U-004 | `crates/infynitae-backend-headless/tests/t0_hot_path_allocations.rs` | `Counting::dealloc` | 2 | test | Free a block from `System`. | The caller guarantees `ptr` came from this allocator with `layout` (`GlobalAlloc::dealloc`'s contract); this allocator obtained it from `System` with the same layout. | The block returns to `System`. | Not counted; nothing else happens. | As U-002. |
