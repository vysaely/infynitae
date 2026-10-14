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
3. Core now tracks an internal `InputClock`: it records the latest timestamp accepted from a real physical device event. Synthesized events query `self.input.clock.latest()`.
4. This guarantees that synthesized events never precede queued hardware events and never jump backward.

## Unverified Hypotheses
- Hypothesized that wrapping at $2^{32}-1$ milliseconds behaves uniformly across all client toolkits; verified in unit tests for 32-bit wrapping arithmetic.

## Performance & Correctness Implications
- Eliminates clock-domain desynchronization between system wall-clock time and physical `libinput` evdev timestamps.
- Preserves full determinism in headless test suites without querying host system clocks.

## Limitations & Counterexamples
- Before any physical input event is accepted by Core, the initial timestamp is `0`, which no valid subsequent event precedes.

## Reproduction Instructions
Execute the adversarial pointer clock verification tests:
```bash
cargo test -p infynitae-core --lib the_input_clock_never_goes_back_and_wraps_like_the_wire
cargo test -p infynitae-backend-headless --test t1_inc7_adversarial audit_finding_a5_synthesised_pointer_events
```

## Links & Citations
- Source Commit: [`e40eb70`](file:///home/vysaely/infynitae-native) (*"fix(input): a synthesised pointer event carries the input clock's time, never 0 (A-5)"*)
- Related Pitfall: [KNOW-PITFALL-004: Mixed Timestamp Clock Domains](../pitfalls/KNOW-PITFALL-004-mixed-timestamp-clock-domains.md)
