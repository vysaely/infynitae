# Unsafe inventory (C9, INV-ARCH-2)

Every `unsafe` site of the adapter and the backends — production code, integration tests, benches
and examples — is listed here, and so is every site of Core's integration tests.
`tools/verify-architecture` matches each site against exactly one row (file, enclosing item,
number of sites, class) and fails on any site that is not listed, on any row without a site, and
on a row of the wrong class. A `test` row never authorises `production` code.

`infynitae-core` forbids `unsafe` in its library, examples, benches and build script, and
`infynitae-ipc` everywhere; no row can change that. Core's integration tests are separate crates
that never link into Core's library; a site there needs a `test` row like any other (owner
decision of 2026-10-14, for the OB-42 counting allocator only).

Production sites: **none.**

| id | file | item | sites | class | purpose | safety preconditions | ownership assumptions | cleanup guarantees | verification |
|---|---|---|---|---|---|---|---|---|---|
| U-002 | `crates/infynitae-core/tests/t0_hot_path_allocations.rs` | `<file>` | 1 | test | `unsafe impl GlobalAlloc for Counting`: the test binary's global allocator, which counts the allocations made on a thread while that thread counts (OB-42 ratchet, INV-ALLOC-1). | The implementation upholds `GlobalAlloc`'s contract because every method forwards its arguments unchanged to `std::alloc::System` and returns its result; `realloc` and `alloc_zeroed` are the trait's defaults, which call these. The counter is a `thread_local!` `Cell<Option<u64>>` with a constant initialiser and no destructor: reading or writing it neither allocates nor unwinds, and `try_with` tolerates a thread being torn down. | Only this test binary uses the allocator; Core's library and every other binary keep the system allocator. No Core state, port or authority is touched. | Nothing to release: the allocator owns no memory; every block belongs to `System`. | `hot_path_allocations_do_not_grow`: the counts are deterministic, equal at 10, 100 and 1,000 windows, and equal to those of `main` (`ccb82c5`) measured with the same file; the same file fails on `40ddd7d` (P-1). |
| U-003 | `crates/infynitae-core/tests/t0_hot_path_allocations.rs` | `Counting::alloc` | 2 | test | Count the allocation, then allocate from `System`. | The caller of `alloc` guarantees a `layout` of non-zero size (`GlobalAlloc::alloc`'s contract); it is passed to `System.alloc` unchanged. | The returned block belongs to the caller, as from `System`. | Freed through `Counting::dealloc`, i.e. by `System`. | As U-002. |
| U-004 | `crates/infynitae-core/tests/t0_hot_path_allocations.rs` | `Counting::dealloc` | 2 | test | Free a block from `System`. | The caller guarantees `ptr` came from this allocator with `layout` (`GlobalAlloc::dealloc`'s contract); this allocator obtained it from `System` with the same layout. | The block returns to `System`. | Not counted; nothing else happens. | As U-002. |
