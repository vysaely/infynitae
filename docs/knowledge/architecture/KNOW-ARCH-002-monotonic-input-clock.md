# KNOW-ARCH-002: Monotonic Input Clock for Synthesized Pointer Events

| Field | Value |
| :--- | :--- |
| **id** | KNOW-ARCH-002 |
| **title** | Monotonic Input Clock for Synthesized Pointer Events |
| **subsystem** | Core/Input |
| **source_rev** | e40eb70 |
| **date** | 2026-10-14 |
| **evidence_class** | EXPERIMENTALLY VERIFIED |
| **applicability** | All Phase F / INC-7+ branches |
| **affected_branches** | experimental/combined-perf-audit-2026-10-14, experimental/inc7-exit-readiness-2026-10-14 |
| **related_invariants** | INV-INPUT-5, M10 |
| **adoption_status** | IMPLEMENTED |
| **next_action** | Maintain monotonic clock ratchet in t1_inc7_adversarial |

---

## Verified Facts
1. Wayland clients require non-decreasing monotonic timestamps on `wl_pointer` and `wl_keyboard` wire events.
2. In early Infynitae implementations (`782a3d3`), synthesized pointer events (`Core::true_position(0)`) passed timestamp `0` during output reconfigurations and presentation switches, violating client monotonicity expectations (Finding A-5).
3. Core now tracks an internal `InputClock` (`input/mod.rs`): `accepted(time_us)` keeps the maximum device time of any accepted physical event; `true_position()` stamps every synthesised pointer event with `self.input.clock.wire_ms()`, and no caller passes a time. The wire conversion `(µs / 1000) as u32` wraps modulo 2³² ms.
4. A synthesised event therefore never precedes an event already delivered and never precedes a hardware event queued while Core synthesised it (device clocks are monotonic and delivered in order).
5. Clock domains: the nested backend's input times are Smithay `Clock<Monotonic>` readings (absolute `CLOCK_MONOTONIC`, µs); libinput uses `CLOCK_MONOTONIC` too (not exercised before INC-13). The session clock (ms since session start) is a different base and is never used for wire times.
6. Separate-process evidence (`fs_client` against `app_probe`): motion times 0, 0, 1040, 0, 1060 before the fix; 1020, 1020, 1040, 1040, 1060 after.

## Unverified Hypotheses
- That every client toolkit handles the 2³² ms wrap uniformly is not verified; the unit test only verifies Core's wrapping arithmetic.

## Performance & Correctness Implications
- No clock other than the input devices' own is mixed into wire times (no session-clock or presentation-clock value reaches a `wl_pointer` time).
- Preserves full determinism in headless test suites without querying host system clocks.

## Limitations & Counterexamples
- Before any physical input event is accepted by Core, the initial timestamp is `0`, which no valid subsequent event precedes.
- **Far-future residual:** one device event stamped far ahead (review probe R4: 1 h ahead) holds every later synthesised time at that value until real time passes it; the device's own stream already went back at the next real event, the synthesised events repeat that reversal. Bounded by device-clock correctness.
- UNRATIFIED as part of INC-7 (A-5 touches ratified INC-2/INC-5 code; no sealed text changes). Ported to `integration/inc-7-exit-2026-10-14` as `d70c3be`.

## Reproduction Instructions
Execute the adversarial pointer clock verification tests:
```bash
cargo test -p infynitae-core --lib the_input_clock_never_goes_back_and_wraps_like_the_wire
cargo test -p infynitae-backend-headless --test t1_inc7_adversarial pointer_event_times_never_go_back
```

## Links & Citations
- Source Commit: [`e40eb70`](file:///home/vysaely/infynitae-native) (*"fix(input): a synthesised pointer event carries the input clock's time, never 0 (A-5)"*)
- Related Pitfall: [KNOW-PITFALL-004: Mixed Timestamp Clock Domains](../pitfalls/KNOW-PITFALL-004-mixed-timestamp-clock-domains.md)
