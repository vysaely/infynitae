# KNOW-PITFALL-004: Mixed Timestamp Clock Domains (Adapter Wall-Clock Extrapolation)

| Field | Value |
| :--- | :--- |
| **id** | KNOW-PITFALL-004 |
| **title** | Mixed Timestamp Clock Domains (Adapter Wall-Clock Extrapolation) |
| **subsystem** | Pitfalls/TimeSynchronization |
| **source_rev** | AGY Comparative Audit / Infynitae INC-7 Review |
| **date** | 2026-10-14 |
| **evidence_class** | CONTRADICTED |
| **applicability** | Pointer and keyboard event timestamping |
| **affected_branches** | N/A (Rejected Anti-Pattern) |
| **related_invariants** | INV-INPUT-5, M10 |
| **adoption_status** | REJECTED |
| **next_action** | Maintain Core InputClock tracking latest physical hardware time |

---

## Verified Facts
1. AGY recommended that Infynitae's Wayland adapter query `smithay::utils::Clock::<Monotonic>::new().now()` to synthesize pointer timestamps during presentation switches.
2. In reality, querying host wall-clock time in the adapter creates a dangerous race condition: if internal transitions run while real `evdev` kernel events are buffered in the socket queue, synthesized events receive time $T_{\text{now}}$ and queued events receive $T_{\text{kernel}}$ ($T_{\text{kernel}} < T_{\text{now}}$), causing backward timestamp jumps on the Wayland wire.
3. Client toolkits (Qt, Chromium, GTK) assert that pointer timestamps never decrease, triggering crashes or desynchronization upon backward time jumps.
4. Infynitae correctly rejected this approach and implemented `InputClock` in Core commit [`e40eb70`](file:///home/<user>/infynitae-native).

5. **Reconciliation 2026-10-14 (main `f323c2d`):** **Corrections (X-09).** Fact 2's backward jump is reasoned from event-loop ordering, not observed: `fs_client` evidence shows the A-5 fix (times 0 → input-clock times), not the wall-clock race. Fact 3 ("Qt, Chromium, GTK assert … triggering crashes") has no source; treat as unverified. The rejection of adapter wall-clock time stands on KNOW-ARCH-002 (one clock for wire times, deterministic tests).

## Unverified Hypotheses
- None; confirmed on the wire with adversarial client `fs_client` in `t1_inc7_adversarial`.

## Performance & Correctness Implications
- Wall-clock querying ruins test determinism and breaks protocol monotonicity.

## Limitations & Counterexamples
- Reconsideration would only be justified if an operating system had zero hardware timestamps available from device drivers.

## Reproduction Instructions
Review commit `e40eb70` commit message and tests:
```bash
git show e40eb70
```

## Links & Citations
- Related Architecture: [KNOW-ARCH-002: Monotonic Input Clock](../architecture/KNOW-ARCH-002-monotonic-input-clock.md)
